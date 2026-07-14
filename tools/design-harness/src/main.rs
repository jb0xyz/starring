mod client;
mod config;
mod eval;
mod store;

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use design_harness::{
    parse_turn_brief, BurstOutcome, DesignSession, Draft, DraftSummary, HaltReport,
    IntentRecipeStatusV1, LimitKind, LlmClient, LlmError, LlmResponse, Message, Observability,
    ResourceBindingMap, SessionConfig, SessionSnapshot, SessionSnapshotError, StructuredError,
    ToolCall, ToolDefinition, TurnIntent,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{self, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::client::GemmaClient;
use crate::config::{
    intent_bindings_from_env, EdgeConfig, HarnessMode, PersistenceConfig, SERVING_MODEL,
};
use crate::store::{SessionStore, StoreError};

const BUILD_SOURCE_COMMIT: &str = env!("STARRING_BUILD_SOURCE_COMMIT");
const BUILD_SOURCE_DIRTY: &str = env!("STARRING_BUILD_SOURCE_DIRTY");

fn create_interactive_session<C>(
    client: C,
    config: SessionConfig,
    snapshot: Option<SessionSnapshot>,
    mode: HarnessMode,
    bindings: Option<ResourceBindingMap>,
) -> Result<DesignSession<C>, SessionSnapshotError> {
    match (snapshot, mode) {
        (Some(snapshot), HarnessMode::Adaptive) => DesignSession::restore(client, config, snapshot),
        (Some(snapshot), HarnessMode::TypedPlan) => {
            DesignSession::restore_planned(client, config, snapshot)
        }
        (Some(snapshot), HarnessMode::IntentRecipe) => DesignSession::restore_intent_recipe(
            client,
            config,
            snapshot,
            required_intent_bindings(bindings)?,
        ),
        (None, HarnessMode::Adaptive) => Ok(DesignSession::with_adaptive_config(client, config)),
        (None, HarnessMode::TypedPlan) => Ok(DesignSession::with_planned_config(client, config)),
        (None, HarnessMode::IntentRecipe) => Ok(DesignSession::with_intent_recipe_config(
            client,
            config,
            required_intent_bindings(bindings)?,
        )),
    }
}

fn required_intent_bindings(
    bindings: Option<ResourceBindingMap>,
) -> Result<ResourceBindingMap, SessionSnapshotError> {
    bindings.ok_or_else(|| SessionSnapshotError::InvalidInvariant {
        message: "intent recipe mode requires the configured resource bindings".to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = EdgeConfig::from_env()?;
    if env::args().nth(1).as_deref() == Some("--eval-json") {
        return run_eval(config).await;
    }
    let persistence = PersistenceConfig::from_env()?;
    let client = match persistence.mode {
        HarnessMode::IntentRecipe => {
            let client = GemmaClient::new_intent_serving(config.base_url, config.api_key)?;
            client.preflight_model().await?;
            client
        }
        HarnessMode::Adaptive | HarnessMode::TypedPlan => {
            GemmaClient::new(config.base_url, config.api_key, config.model)?
        }
    };
    let mut store = SessionStore::open(&persistence.db_path)?;
    let loaded = store.load_versioned(&persistence.session_id)?;
    let mut generation = loaded.as_ref().map_or(0, |loaded| loaded.generation);
    let mut session = create_interactive_session(
        client.clone(),
        config.session_config.clone(),
        loaded.map(|loaded| loaded.snapshot),
        persistence.mode,
        persistence.bindings.clone(),
    )?;
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
        generation = match store.save_compare_and_swap(
            &persistence.session_id,
            generation,
            &session.snapshot(),
        ) {
            Ok(generation) => generation,
            Err(StoreError::GenerationConflict { .. }) => {
                let loaded = store.load_versioned(&persistence.session_id)?;
                generation = loaded.as_ref().map_or(0, |loaded| loaded.generation);
                session = create_interactive_session(
                    client.clone(),
                    config.session_config.clone(),
                    loaded.map(|loaded| loaded.snapshot),
                    persistence.mode,
                    persistence.bindings.clone(),
                )?;
                write_line(
                    &mut output,
                    "conflict> session changed concurrently; submit the request again",
                )
                .await?;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
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
            BurstOutcome::Routed { fallback, .. } => {
                write_line(&mut output, &format!("assistant> {}", fallback.response())).await?;
            }
            BurstOutcome::Halted(report) => {
                write_halt(&mut output, &report).await?;
                break;
            }
        }
    }
    Ok(())
}

async fn run_eval(config: EdgeConfig) -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).await?;
    let scenario = parse_eval_input(&input)?;
    let document = match scenario.mode {
        EvalMode::IntentRecipe => {
            let provenance = IntentEvalProvenance::from_env()?;
            let bindings = intent_bindings_from_env()?;
            let client = GemmaClient::new_intent_serving(config.base_url, config.api_key)?;
            client.preflight_model().await?;
            execute_intent_eval(
                client,
                config.session_config,
                bindings,
                scenario,
                provenance,
            )
            .await?
        }
        EvalMode::Adaptive | EvalMode::TypedPlan => {
            let client = GemmaClient::new(config.base_url, config.api_key, config.model)?;
            execute_legacy_eval(client, config.session_config, scenario).await?
        }
    };
    let mut output = io::stdout();
    output
        .write_all(serde_json::to_string(&document)?.as_bytes())
        .await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}

async fn execute_legacy_eval(
    client: GemmaClient,
    config: SessionConfig,
    scenario: EvalScenario,
) -> Result<Value, Box<dyn Error>> {
    let oracle = Arc::new(Mutex::new(OracleState::default()));
    let client = EvalClient {
        inner: client,
        oracle: Arc::clone(&oracle),
    };
    let legacy_plan_enabled = scenario.turns.iter().any(|turn| turn.oracle_plan.is_some());
    let mut session = match scenario.mode {
        EvalMode::Adaptive => DesignSession::with_adaptive_config(client, config),
        EvalMode::TypedPlan if legacy_plan_enabled => {
            DesignSession::with_planned_oracle_config(client, config)
        }
        EvalMode::TypedPlan => DesignSession::with_planned_config(client, config),
        EvalMode::IntentRecipe => return Err("intent recipe evaluation entered legacy path".into()),
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
        prepare_oracle_controls(
            &oracle,
            turn.oracle_brief.as_ref(),
            turn.oracle_plan.as_ref(),
        )?;
        let turn_started = Instant::now();
        let outcome = session.run_burst(&turn.input).await;
        clear_oracle_controls(&oracle)?;
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
    Ok(eval::report(
        &session,
        reports,
        started.elapsed(),
        scenario.schema_version,
        scenario.mode.as_str(),
        injected_control_calls(&oracle)?,
        delegated_model_calls(&oracle)?,
    )
    .await)
}

async fn execute_intent_eval<C>(
    client: C,
    config: SessionConfig,
    bindings: ResourceBindingMap,
    scenario: EvalScenario,
    provenance: IntentEvalProvenance,
) -> Result<Value, Box<dyn Error>>
where
    C: LlmClient + Clone,
{
    if scenario.schema_version != 3 || scenario.mode != EvalMode::IntentRecipe {
        return Err(
            "intent recipe evaluation requires schema_version 3 intent_recipe input".into(),
        );
    }
    if scenario.initial_draft.is_some()
        || scenario
            .turns
            .iter()
            .any(|turn| turn.oracle_brief.is_some() || turn.oracle_plan.is_some())
    {
        return Err("intent recipe evaluation cannot contain legacy or oracle state".into());
    }
    let started_at_unix_ms = unix_timestamp_millis()?;
    let started = Instant::now();
    let turn_count = scenario.turns.len();
    let store_path = intent_eval_store_path(provenance.run_order, started_at_unix_ms)?;
    let _store_cleanup = EvalStoreCleanup::new(store_path.clone());
    let session_id = format!("intent-eval-{}", provenance.run_order);
    let mut store = Some(SessionStore::open(&store_path)?);
    let mut generation = 0u64;
    let mut store_writes = 0usize;
    let mut session =
        DesignSession::with_intent_recipe_config(client.clone(), config.clone(), bindings.clone());
    let mut reports = Vec::with_capacity(turn_count);
    let mut connection_reopens = 0usize;
    for (index, turn) in scenario.turns.into_iter().enumerate() {
        let draft_before = session.draft().clone();
        let observability_before = session.observability().clone();
        let status_before = required_intent_status(&session)?;
        let turn_started = Instant::now();
        let outcome = session.run_burst(&turn.input).await;
        let elapsed = turn_started.elapsed();
        let status_after = required_intent_status(&session)?;
        let route_decision = match &outcome {
            BurstOutcome::Routed { decision, .. } => Some(decision.as_ref().clone()),
            _ => session.intent_recipe_route_decision().cloned(),
        };
        generation = store
            .as_mut()
            .ok_or("intent evaluation store is unavailable")?
            .save_compare_and_swap(&session_id, generation, &session.snapshot())?;
        store_writes = store_writes.saturating_add(1);
        let restart_requested = turn.restart_after
            && index + 1 < turn_count
            && !matches!(outcome, BurstOutcome::Halted(_));
        let restart_performed = if restart_requested {
            drop(store.take());
            let reopened = SessionStore::open(&store_path)?;
            let loaded = reopened
                .load_versioned(&session_id)?
                .ok_or("intent evaluation snapshot disappeared during reopen")?;
            if loaded.generation != generation {
                return Err("intent evaluation generation changed during reopen".into());
            }
            session = DesignSession::restore_intent_recipe(
                client.clone(),
                config.clone(),
                loaded.snapshot,
                bindings.clone(),
            )?;
            store = Some(reopened);
            connection_reopens = connection_reopens.saturating_add(1);
            true
        } else {
            false
        };
        reports.push(eval::intent_turn_report(eval::IntentTurnReportInput {
            id: &turn.id,
            input: &turn.input,
            before: &draft_before,
            after: session.draft(),
            status_before: &status_before,
            status_after: &status_after,
            observability_before: &observability_before,
            observability_after: session.observability(),
            outcome: &outcome,
            route_decision: route_decision.as_ref(),
            elapsed,
            restart_after: turn.restart_after,
            restart_performed,
        }));
        if matches!(outcome, BurstOutcome::Halted(_)) {
            break;
        }
    }
    let ended_at_unix_ms = unix_timestamp_millis()?;
    Ok(eval::intent_report(
        &session,
        reports,
        started.elapsed(),
        eval::IntentPersistenceEvidence {
            store_writes,
            connection_reopens,
            final_generation: generation,
        },
        eval::IntentReportMetadata {
            gateway_id: &provenance.gateway_id,
            declared_context_tokens: provenance.declared_context_tokens,
            source_commit: &provenance.source_commit,
            source_dirty: provenance.source_dirty,
            build_source_commit: BUILD_SOURCE_COMMIT,
            build_source_dirty: BUILD_SOURCE_DIRTY == "true",
            binary_sha256: &provenance.binary_sha256,
            run_id: &provenance.run_id,
            run_order: provenance.run_order,
            started_at_unix_ms,
            ended_at_unix_ms,
            requested_model: SERVING_MODEL,
            session_config: &config,
        },
    ))
}

struct EvalStoreCleanup {
    path: PathBuf,
}

impl EvalStoreCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for EvalStoreCleanup {
    fn drop(&mut self) {
        for path in [
            self.path.clone(),
            PathBuf::from(format!("{}-shm", self.path.display())),
            PathBuf::from(format!("{}-wal", self.path.display())),
        ] {
            let _ = fs::remove_file(path);
        }
    }
}

fn intent_eval_store_path(
    run_order: u64,
    started_at_unix_ms: u64,
) -> Result<PathBuf, Box<dyn Error>> {
    let path = env::temp_dir().join(format!(
        "starring-intent-eval-{}-{run_order}-{started_at_unix_ms}.sqlite3",
        std::process::id()
    ));
    if path.exists() {
        return Err("intent evaluation SQLite path already exists".into());
    }
    Ok(path)
}

fn required_intent_status<C>(
    session: &DesignSession<C>,
) -> Result<IntentRecipeStatusV1, Box<dyn Error>> {
    session
        .intent_recipe_status()
        .ok_or_else(|| "intent recipe evaluation session lost its public status".into())
}

struct IntentEvalProvenance {
    gateway_id: String,
    declared_context_tokens: u64,
    source_commit: String,
    source_dirty: bool,
    binary_sha256: String,
    run_id: String,
    run_order: u64,
}

impl IntentEvalProvenance {
    fn from_env() -> Result<Self, Box<dyn Error>> {
        let provenance = intent_eval_provenance_from(|name| env::var(name).ok())?;
        verify_intent_eval_artifact(
            &provenance,
            BUILD_SOURCE_COMMIT,
            BUILD_SOURCE_DIRTY,
            &current_binary_sha256()?,
        )?;
        Ok(provenance)
    }
}

fn intent_eval_provenance_from<F>(mut value: F) -> Result<IntentEvalProvenance, Box<dyn Error>>
where
    F: FnMut(&str) -> Option<String>,
{
    let gateway_id = required_eval_identifier(
        value("STARRING_EVAL_GATEWAY_ID"),
        "STARRING_EVAL_GATEWAY_ID",
        128,
    )?;
    if gateway_id.contains("://") {
        return Err("STARRING_EVAL_GATEWAY_ID must be an opaque identity, not a URL".into());
    }
    let declared_context_tokens = required_eval_value(
        value("STARRING_EVAL_DECLARED_CONTEXT_TOKENS"),
        "STARRING_EVAL_DECLARED_CONTEXT_TOKENS",
    )?
    .parse::<u64>()
    .map_err(|_| "STARRING_EVAL_DECLARED_CONTEXT_TOKENS must be 16384")?;
    if declared_context_tokens != 16_384 {
        return Err("STARRING_EVAL_DECLARED_CONTEXT_TOKENS must be 16384".into());
    }
    let source_commit = required_eval_value(
        value("STARRING_EVAL_SOURCE_COMMIT"),
        "STARRING_EVAL_SOURCE_COMMIT",
    )?;
    if !matches!(source_commit.len(), 40 | 64)
        || !source_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(
            "STARRING_EVAL_SOURCE_COMMIT must be a 40 or 64 character hexadecimal commit identity"
                .into(),
        );
    }
    let source_dirty = match required_eval_value(
        value("STARRING_EVAL_SOURCE_DIRTY"),
        "STARRING_EVAL_SOURCE_DIRTY",
    )?
    .as_str()
    {
        "true" => true,
        "false" => false,
        _ => return Err("STARRING_EVAL_SOURCE_DIRTY must be true or false".into()),
    };
    let binary_sha256 = required_eval_hash(
        value("STARRING_EVAL_BINARY_SHA256"),
        "STARRING_EVAL_BINARY_SHA256",
    )?;
    let run_id =
        required_eval_identifier(value("STARRING_EVAL_RUN_ID"), "STARRING_EVAL_RUN_ID", 128)?;
    let run_order =
        required_eval_value(value("STARRING_EVAL_RUN_ORDER"), "STARRING_EVAL_RUN_ORDER")?
            .parse::<u64>()
            .ok()
            .filter(|value| (1..=9_007_199_254_740_991).contains(value))
            .ok_or("STARRING_EVAL_RUN_ORDER must be a positive JSON-safe integer")?;
    Ok(IntentEvalProvenance {
        gateway_id,
        declared_context_tokens,
        source_commit,
        source_dirty,
        binary_sha256,
        run_id,
        run_order,
    })
}

fn required_eval_hash(value: Option<String>, name: &str) -> Result<String, Box<dyn Error>> {
    let value = required_eval_value(value, name)?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{name} must be a 64 character lowercase hexadecimal digest").into());
    }
    Ok(value)
}

fn current_binary_sha256() -> Result<String, Box<dyn Error>> {
    let executable = env::current_exe()?;
    binary_sha256(&executable)
}

fn binary_sha256(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path)?);
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_intent_eval_artifact(
    provenance: &IntentEvalProvenance,
    build_commit: &str,
    build_dirty: &str,
    actual_binary_sha256: &str,
) -> Result<(), Box<dyn Error>> {
    if provenance.source_commit != build_commit {
        return Err("intent evaluation binary was built from a different source commit".into());
    }
    let build_dirty = match build_dirty {
        "true" => true,
        "false" => false,
        _ => return Err("intent evaluation binary has invalid build source metadata".into()),
    };
    if provenance.source_dirty != build_dirty {
        return Err("intent evaluation build and runner dirty states differ".into());
    }
    if provenance.binary_sha256 != actual_binary_sha256 {
        return Err("intent evaluation provider and child binary digests differ".into());
    }
    Ok(())
}

fn required_eval_value(value: Option<String>, name: &str) -> Result<String, Box<dyn Error>> {
    let value = value.ok_or_else(|| format!("{name} is required"))?;
    if value.trim().is_empty() || value != value.trim() {
        return Err(format!("{name} must be a non-empty trimmed value").into());
    }
    Ok(value)
}

fn required_eval_identifier(
    value: Option<String>,
    name: &str,
    max_len: usize,
) -> Result<String, Box<dyn Error>> {
    let value = required_eval_value(value, name)?;
    if value.len() > max_len
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
    {
        return Err(format!("{name} contains unsupported identity characters").into());
    }
    Ok(value)
}

fn unix_timestamp_millis() -> Result<u64, Box<dyn Error>> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch")?
        .as_millis();
    u64::try_from(millis).map_err(|_| "system clock timestamp does not fit u64".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvalMode {
    Adaptive,
    TypedPlan,
    IntentRecipe,
}

impl EvalMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::TypedPlan => "typed_plan",
            Self::IntentRecipe => "intent_recipe",
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
struct LegacyEvalInput {
    schema_version: u32,
    #[serde(default)]
    mode: Option<EvalMode>,
    #[serde(default)]
    initial_draft: Option<Draft>,
    turns: Vec<LegacyEvalTurn>,
}

#[derive(Debug)]
struct EvalTurn {
    id: String,
    input: String,
    oracle_brief: Option<Value>,
    oracle_plan: Option<Value>,
    restart_after: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyEvalTurn {
    id: String,
    input: String,
    #[serde(default)]
    oracle_brief: Option<Value>,
    #[serde(default)]
    oracle_plan: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum IntentEvalMode {
    IntentRecipe,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentEvalInputV3 {
    schema_version: u32,
    mode: IntentEvalMode,
    turns: Vec<IntentEvalTurnV3>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentEvalTurnV3 {
    id: String,
    input: String,
    #[serde(default)]
    restart_after: bool,
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
                oracle_brief: None,
                oracle_plan: None,
                restart_after: false,
            }],
        });
    }

    let envelope: Value = serde_json::from_str(input)?;
    let schema_version = envelope
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or("evaluation input requires an integer schema_version")?;
    if schema_version == 3 {
        return parse_intent_eval_input(input);
    }
    let document: LegacyEvalInput = serde_json::from_str(input)?;
    let mode = match document.schema_version {
        1 => {
            if document.mode.is_some()
                || document.initial_draft.is_some()
                || document
                    .turns
                    .iter()
                    .any(|turn| turn.oracle_brief.is_some() || turn.oracle_plan.is_some())
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
        validate_oracle_turn(turn)?;
    }
    if let Some(draft) = document.initial_draft.as_ref() {
        validate_initial_draft(draft)?;
    }
    Ok(EvalScenario {
        schema_version: document.schema_version,
        mode,
        initial_draft: document.initial_draft,
        turns: document
            .turns
            .into_iter()
            .map(|turn| EvalTurn {
                id: turn.id,
                input: turn.input,
                oracle_brief: turn.oracle_brief,
                oracle_plan: turn.oracle_plan,
                restart_after: false,
            })
            .collect(),
    })
}

fn parse_intent_eval_input(input: &str) -> Result<EvalScenario, Box<dyn Error>> {
    let document: IntentEvalInputV3 = serde_json::from_str(input)?;
    if document.schema_version != 3 {
        return Err("intent recipe evaluation requires schema_version 3".into());
    }
    if document.mode != IntentEvalMode::IntentRecipe {
        return Err("evaluation schema_version 3 requires mode intent_recipe".into());
    }
    if document.turns.is_empty() {
        return Err("evaluation input must contain at least one turn".into());
    }
    let mut ids = BTreeSet::new();
    for turn in &document.turns {
        validate_eval_turn_identity(&turn.id, &turn.input, &mut ids)?;
    }
    Ok(EvalScenario {
        schema_version: 3,
        mode: EvalMode::IntentRecipe,
        initial_draft: None,
        turns: document
            .turns
            .into_iter()
            .map(|turn| EvalTurn {
                id: turn.id,
                input: turn.input,
                oracle_brief: None,
                oracle_plan: None,
                restart_after: turn.restart_after,
            })
            .collect(),
    })
}

fn validate_eval_turn_identity<'a>(
    id: &'a str,
    input: &str,
    ids: &mut BTreeSet<&'a str>,
) -> Result<(), Box<dyn Error>> {
    if id.trim().is_empty() {
        return Err("evaluation turn id must not be empty".into());
    }
    if !ids.insert(id) {
        return Err(format!("duplicate evaluation turn id {id}").into());
    }
    if input.trim().is_empty() {
        return Err(format!("evaluation turn {id} input must not be empty").into());
    }
    Ok(())
}

fn validate_oracle_turn(turn: &LegacyEvalTurn) -> Result<(), Box<dyn Error>> {
    let Some(brief) = turn.oracle_brief.as_ref() else {
        if turn.oracle_plan.is_some() {
            return Err(format!(
                "evaluation turn {} oracle_plan requires oracle_brief",
                turn.id
            )
            .into());
        }
        return Ok(());
    };
    let arguments = serde_json::to_string(brief)?;
    let brief = parse_turn_brief(&arguments).map_err(|error| {
        format!(
            "evaluation turn {} oracle_brief is invalid: {}",
            turn.id, error.message
        )
    })?;
    match (brief.intent, turn.oracle_plan.is_some()) {
        (TurnIntent::Build, false) => Err(format!(
            "evaluation turn {} build oracle_brief requires oracle_plan",
            turn.id
        )
        .into()),
        (TurnIntent::Build, true) => Ok(()),
        (_, true) => Err(format!(
            "evaluation turn {} oracle_plan requires build oracle_brief",
            turn.id
        )
        .into()),
        (_, false) => Ok(()),
    }
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum OracleStage {
    #[default]
    Inactive,
    BriefPending,
    PlanPending,
    Complete,
}

#[derive(Default)]
struct OracleState {
    stage: OracleStage,
    current_brief: Option<String>,
    current_plan: Option<String>,
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
        {
            let mut oracle = self
                .oracle
                .lock()
                .map_err(|_| LlmError::Client("oracle control state is unavailable".to_string()))?;
            let expected = match oracle.stage {
                OracleStage::BriefPending => Some("set_turn_brief"),
                OracleStage::PlanPending => Some("set_turn_plan"),
                OracleStage::Inactive | OracleStage::Complete => None,
            };
            if let Some(name) = expected {
                if tools.len() != 1 || tools[0].name != name {
                    return Err(LlmError::Client(format!(
                        "oracle benchmark expected sole {name} frontier"
                    )));
                }
                let arguments = match oracle.stage {
                    OracleStage::BriefPending => {
                        let arguments = oracle.current_brief.take();
                        oracle.stage = if oracle.current_plan.is_some() {
                            OracleStage::PlanPending
                        } else {
                            OracleStage::Complete
                        };
                        arguments
                    }
                    OracleStage::PlanPending => {
                        oracle.stage = OracleStage::Complete;
                        oracle.current_plan.take()
                    }
                    OracleStage::Inactive | OracleStage::Complete => None,
                }
                .ok_or_else(|| {
                    LlmError::Client(format!("oracle benchmark missing configured {name}"))
                })?;
                oracle.injected_control_calls += 1;
                return Ok(LlmResponse::ToolCalls(vec![ToolCall {
                    id: format!("eval_oracle_{}", oracle.injected_control_calls),
                    name: name.to_string(),
                    arguments,
                }]));
            }
            if oracle.stage == OracleStage::Complete
                && tools
                    .iter()
                    .any(|tool| matches!(tool.name.as_str(), "set_turn_brief" | "set_turn_plan"))
            {
                return Err(LlmError::Client(
                    "oracle benchmark refused repeated control delegation".to_string(),
                ));
            }
            oracle.delegated_model_calls += 1;
        }
        self.inner.complete(messages, tools).await
    }
}

fn prepare_oracle_controls(
    oracle: &Arc<Mutex<OracleState>>,
    brief: Option<&Value>,
    plan: Option<&Value>,
) -> Result<(), Box<dyn Error>> {
    if brief.is_none() && plan.is_some() {
        return Err("oracle_plan requires oracle_brief".into());
    }
    let mut oracle = oracle
        .lock()
        .map_err(|_| "oracle control state is unavailable")?;
    if oracle.stage != OracleStage::Inactive {
        return Err("oracle controls cannot be reset before the current turn is cleared".into());
    }
    oracle.current_brief = brief.map(serde_json::to_string).transpose()?;
    oracle.current_plan = plan.map(serde_json::to_string).transpose()?;
    oracle.stage = if brief.is_some() {
        OracleStage::BriefPending
    } else {
        OracleStage::Inactive
    };
    Ok(())
}

fn clear_oracle_controls(oracle: &Arc<Mutex<OracleState>>) -> Result<(), Box<dyn Error>> {
    let mut oracle = oracle
        .lock()
        .map_err(|_| "oracle control state is unavailable")?;
    if matches!(
        oracle.stage,
        OracleStage::BriefPending | OracleStage::PlanPending
    ) {
        return Err("oracle turn ended with unconsumed configured controls".into());
    }
    oracle.current_brief = None;
    oracle.current_plan = None;
    oracle.stage = OracleStage::Inactive;
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
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use design_harness::{
        simulate_draft, validate_draft, BurstOutcome, DesignSession, Draft, LlmClient, LlmError,
        LlmResponse, Message, ResourceBindingMap, SessionConfig, ToolCall, ToolDefinition,
    };
    use serde_json::json;

    use super::{
        clear_oracle_controls, create_interactive_session, delegated_model_calls,
        execute_intent_eval, injected_control_calls, intent_eval_provenance_from, parse_eval_input,
        prepare_oracle_controls, verify_intent_eval_artifact, EvalClient, EvalMode, HarnessMode,
        IntentEvalProvenance, OracleState,
    };

    struct StubClient;

    struct QueueClient {
        responses: Mutex<VecDeque<LlmResponse>>,
    }

    struct ProbeClient {
        responses: Mutex<VecDeque<LlmResponse>>,
        delegated_calls: Arc<Mutex<usize>>,
    }

    type IntentEvalCalls = Vec<Vec<String>>;

    #[derive(Clone)]
    struct IntentEvalClient {
        responses: Arc<Mutex<VecDeque<LlmResponse>>>,
        calls: Arc<Mutex<IntentEvalCalls>>,
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

    impl LlmClient for IntentEvalClient {
        async fn complete(
            &self,
            _messages: &[Message],
            tools: &[ToolDefinition],
        ) -> Result<LlmResponse, LlmError> {
            self.calls
                .lock()
                .map_err(|_| LlmError::Client("intent eval call log is unavailable".to_string()))?
                .push(tools.iter().map(|tool| tool.name.clone()).collect());
            self.responses
                .lock()
                .map_err(|_| {
                    LlmError::Client("intent eval response queue is unavailable".to_string())
                })?
                .pop_front()
                .ok_or_else(|| LlmError::Client("intent eval response queue is empty".to_string()))
        }
    }

    #[test]
    fn interactive_session_selection_covers_fresh_and_restored_modes() {
        let config = SessionConfig::default();
        let adaptive_fresh = create_interactive_session(
            StubClient,
            config.clone(),
            None,
            HarnessMode::Adaptive,
            None,
        )
        .unwrap();
        let planned_fresh = create_interactive_session(
            StubClient,
            config.clone(),
            None,
            HarnessMode::TypedPlan,
            None,
        )
        .unwrap();
        let intent_fresh = create_interactive_session(
            StubClient,
            config.clone(),
            None,
            HarnessMode::IntentRecipe,
            Some(ResourceBindingMap::default()),
        )
        .unwrap();
        assert!(!adaptive_fresh.planned_enabled());
        assert!(planned_fresh.planned_enabled());
        assert!(intent_fresh.intent_recipe_enabled());

        let snapshot = adaptive_fresh.snapshot();
        let adaptive_restored = create_interactive_session(
            StubClient,
            config.clone(),
            Some(snapshot.clone()),
            HarnessMode::Adaptive,
            None,
        )
        .unwrap();
        let planned_restored = create_interactive_session(
            StubClient,
            config.clone(),
            Some(snapshot),
            HarnessMode::TypedPlan,
            None,
        )
        .unwrap();
        let intent_restored = create_interactive_session(
            StubClient,
            config,
            Some(intent_fresh.snapshot()),
            HarnessMode::IntentRecipe,
            Some(ResourceBindingMap::default()),
        )
        .unwrap();
        assert!(!adaptive_restored.planned_enabled());
        assert!(planned_restored.planned_enabled());
        assert!(intent_restored.intent_recipe_enabled());
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

    fn build_brief(objective: &str) -> serde_json::Value {
        json!({
            "intent": "build",
            "objective": objective,
            "requested_outcome": "draft_update",
            "assumptions": [],
            "validate": false
        })
    }

    fn inspect_brief(objective: &str) -> serde_json::Value {
        json!({
            "intent": "inspect",
            "objective": objective,
            "requested_outcome": "validated_preview",
            "assumptions": [],
            "validate": true
        })
    }

    fn validated_build_brief(objective: &str) -> serde_json::Value {
        json!({
            "intent": "build",
            "objective": objective,
            "requested_outcome": "validated_preview",
            "assumptions": [],
            "validate": true
        })
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
        let responses = VecDeque::from([call(
            "finish",
            "finish_turn",
            r#"{"kind":"progressed","message":"Stage complete"}"#,
        )]);
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        prepare_oracle_controls(
            &oracle,
            Some(&build_brief("Complete the requested stage")),
            Some(&fixtures[plan]),
        )
        .unwrap();
        let client = EvalClient {
            inner: QueueClient {
                responses: Mutex::new(responses),
            },
            oracle: Arc::clone(&oracle),
        };
        let mut session =
            DesignSession::with_planned_oracle_config(client, SessionConfig::default());
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
    fn eval_input_accepts_typed_plan_with_exact_oracle_controls() {
        let scenario = parse_eval_input(
            r#"{"schema_version":2,"mode":"typed_plan","initial_draft":{"ruleset":{"version":1,"panels":[],"modals":[],"rules":[]},"draft_revision":0,"validated_revision":null,"simulated_revision":null},"turns":[{"id":"build","input":"Build it","oracle_brief":{"intent":"build","objective":"Build it","requested_outcome":"draft_update","assumptions":[],"validate":false},"oracle_plan":{"requirements":[]}}]}"#,
        )
        .unwrap();

        assert_eq!(scenario.schema_version, 2);
        assert_eq!(scenario.mode, EvalMode::TypedPlan);
        assert_eq!(scenario.initial_draft.unwrap().draft_revision, 0);
        assert_eq!(
            scenario.turns[0].oracle_brief,
            Some(build_brief("Build it"))
        );
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
            r#"{"schema_version":1,"turns":[{"id":"a","input":"b","oracle_brief":{"intent":"build"}}]}"#,
        )
        .is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":2,"mode":"adaptive","turns":[{"id":"a","input":"b"}]}"#,
        )
        .is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":2,"mode":"typed_plan","turns":[{"id":"a","input":"b","oracle_plan":{"requirements":[]}}]}"#,
        )
        .is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":2,"mode":"typed_plan","turns":[{"id":"a","input":"b","oracle_brief":{"intent":"build","objective":"Build","requested_outcome":"draft_update","assumptions":[],"validate":false}}]}"#,
        )
        .is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":2,"mode":"typed_plan","turns":[{"id":"a","input":"b","oracle_brief":{"intent":"inspect","objective":"Inspect","requested_outcome":"validated_preview","assumptions":[],"validate":true},"oracle_plan":{"requirements":[]}}]}"#,
        )
        .is_err());
        assert!(parse_eval_input(
            r#"{"schema_version":2,"mode":"typed_plan","initial_draft":{"ruleset":{"version":1,"panels":[{"key":"p","channel":"missing","content":"c","buttons":[]}],"modals":[],"rules":[]},"draft_revision":1,"validated_revision":null,"simulated_revision":null},"turns":[{"id":"a","input":"b"}]}"#,
        )
        .is_err());
    }

    #[test]
    fn intent_eval_input_is_strict_and_oracle_free() {
        let scenario = parse_eval_input(
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[{"id":"request","input":"Build private study rooms","restart_after":true},{"id":"hub","input":"Use community_hub"}]}"#,
        )
        .unwrap();

        assert_eq!(scenario.schema_version, 3);
        assert_eq!(scenario.mode, EvalMode::IntentRecipe);
        assert!(scenario.initial_draft.is_none());
        assert_eq!(scenario.turns.len(), 2);
        assert!(scenario.turns[0].restart_after);
        assert!(scenario
            .turns
            .iter()
            .all(|turn| turn.oracle_brief.is_none() && turn.oracle_plan.is_none()));

        for invalid in [
            r#"{"schema_version":3,"mode":"intent_recipe","initial_draft":null,"turns":[{"id":"a","input":"b"}]}"#,
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[{"id":"a","input":"b","oracle_brief":null}]}"#,
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[{"id":"a","input":"b","oracle_plan":null}]}"#,
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[{"id":"a","input":"b","unknown":true}]}"#,
            r#"{"schema_version":3,"mode":"typed_plan","turns":[{"id":"a","input":"b"}]}"#,
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[]}"#,
            r#"{"schema_version":3,"schema_version":3,"mode":"intent_recipe","turns":[{"id":"a","input":"b"}]}"#,
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[{"id":"a","id":"a","input":"b"}]}"#,
        ] {
            assert!(parse_eval_input(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn intent_eval_provenance_is_complete_bounded_and_context_is_declared() {
        let values = BTreeMap::from([
            ("STARRING_EVAL_GATEWAY_ID", "home-gateway"),
            ("STARRING_EVAL_DECLARED_CONTEXT_TOKENS", "16384"),
            (
                "STARRING_EVAL_SOURCE_COMMIT",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            ("STARRING_EVAL_SOURCE_DIRTY", "false"),
            (
                "STARRING_EVAL_BINARY_SHA256",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            ("STARRING_EVAL_RUN_ID", "intent-run-1"),
            ("STARRING_EVAL_RUN_ORDER", "1"),
        ]);
        let parsed =
            intent_eval_provenance_from(|name| values.get(name).map(|value| (*value).to_string()))
                .unwrap();

        assert_eq!(parsed.gateway_id, "home-gateway");
        assert_eq!(parsed.declared_context_tokens, 16_384);
        assert_eq!(
            parsed.source_commit,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(!parsed.source_dirty);
        assert_eq!(parsed.run_id, "intent-run-1");
        assert_eq!(parsed.run_order, 1);
        assert!(verify_intent_eval_artifact(
            &parsed,
            &parsed.source_commit,
            "false",
            &parsed.binary_sha256,
        )
        .is_ok());
        assert!(verify_intent_eval_artifact(
            &parsed,
            "cccccccccccccccccccccccccccccccccccccccc",
            "false",
            &parsed.binary_sha256,
        )
        .is_err());

        for (name, invalid) in [
            ("STARRING_EVAL_GATEWAY_ID", "https://secret.example"),
            ("STARRING_EVAL_DECLARED_CONTEXT_TOKENS", "32768"),
            ("STARRING_EVAL_SOURCE_COMMIT", "not-a-commit"),
            (
                "STARRING_EVAL_SOURCE_COMMIT",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            ),
            (
                "STARRING_EVAL_SOURCE_COMMIT",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            ("STARRING_EVAL_SOURCE_DIRTY", "0"),
            ("STARRING_EVAL_BINARY_SHA256", "short"),
            ("STARRING_EVAL_RUN_ID", "contains whitespace"),
            ("STARRING_EVAL_RUN_ORDER", "0"),
            ("STARRING_EVAL_RUN_ORDER", "9007199254740992"),
        ] {
            assert!(intent_eval_provenance_from(|key| {
                if key == name {
                    Some(invalid.to_string())
                } else {
                    values.get(key).map(|value| (*value).to_string())
                }
            })
            .is_err());
        }
        assert!(intent_eval_provenance_from(|name| {
            (name != "STARRING_EVAL_RUN_ID").then(|| values.get(name).unwrap().to_string())
        })
        .is_err());
    }

    #[tokio::test]
    async fn intent_eval_restarts_without_oracle_and_reports_public_receipt() {
        let responses = VecDeque::from([
            LlmResponse::ToolCalls(vec![ToolCall {
                id: "interpret".to_string(),
                name: "interpret_intent_core".to_string(),
                arguments: json!({
                    "expected_revision": 0,
                    "request_mode": "build",
                    "automation_kind": "managed_private_study_room",
                    "objective": "Create managed private study rooms",
                    "requested_outcome": "validated_preview",
                    "hub_channel": null,
                    "language": "en",
                    "close_policy": "disabled",
                    "runtime_requirements": [],
                    "validation_gate": "enforce",
                    "preview_gate": "enforce",
                    "approval_gate": "enforce",
                    "live_discord_mutation": "no_live_mutation",
                    "secret_disclosure": "no_secret_disclosure",
                    "other_unmapped_required_capabilities": [],
                    "custom_detail_facets": [],
                    "response": ""
                })
                .to_string(),
            }]),
            LlmResponse::ToolCalls(vec![ToolCall {
                id: "resolve".to_string(),
                name: "resolve_intent_decision".to_string(),
                arguments: json!({
                    "expected_revision": 1,
                    "channel": "community_hub"
                })
                .to_string(),
            }]),
        ]);
        let client = IntentEvalClient {
            responses: Arc::new(Mutex::new(responses)),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let probe = client.clone();
        let scenario = parse_eval_input(
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[{"id":"request","input":"Build private study rooms","restart_after":true},{"id":"hub","input":"Use community_hub"}]}"#,
        )
        .unwrap();
        let mut bindings = ResourceBindingMap::default();
        bindings.channel_bindings.insert(
            serde_json::from_value(json!("community_hub")).unwrap(),
            "700".parse().unwrap(),
        );

        let document = execute_intent_eval(
            client,
            SessionConfig::default(),
            bindings,
            scenario,
            IntentEvalProvenance {
                gateway_id: "test-gateway".to_string(),
                declared_context_tokens: 16_384,
                source_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                source_dirty: false,
                binary_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_string(),
                run_id: "intent-eval-test".to_string(),
                run_order: 1,
            },
        )
        .await
        .unwrap();

        assert_eq!(document["schema_version"], 3);
        assert_eq!(document["mode"], "intent_recipe");
        assert_eq!(document["requested_model"], "gemma4:12b-mlx");
        assert_eq!(document["served_model"], "gemma4:12b-mlx");
        assert_eq!(document["declared_context_tokens"], 16_384);
        assert!(document["gateway_context_observed_tokens"].is_null());
        assert_eq!(document["oracle"]["enabled"], false);
        assert_eq!(document["oracle"]["injected_control_calls"], 0);
        assert_eq!(document["persistence"]["backend"], "sqlite_file");
        assert_eq!(document["persistence"]["connection_reopen_count"], 1);
        assert_eq!(document["persistence"]["roundtrip_verified"], true);
        assert_eq!(document["persistence"]["store_writes"], 2);
        assert_eq!(document["persistence"]["final_generation"], 2);
        assert_eq!(document["turns"][0]["stage_before"], "empty");
        assert_eq!(document["turns"][0]["stage_after"], "awaiting_decision");
        assert_eq!(document["turns"][0]["restart_performed"], true);
        assert_eq!(document["turns"][1]["stage_before"], "awaiting_decision");
        assert_eq!(document["turns"][1]["stage_after"], "preview_ready");
        let pending_decision = &document["turns"][0]["route_decision"];
        let resolved_decision = &document["turns"][1]["route_decision"];
        let final_decision = &document["final_intent"]["route_decision"];
        assert_eq!(pending_decision, resolved_decision);
        assert_eq!(resolved_decision, final_decision);
        assert_eq!(pending_decision["kind"], "private_study_room");
        for field in [
            "semantic_ir_digest",
            "manifest_digest",
            "adjudication_digest",
        ] {
            let digest = pending_decision[field].as_str().unwrap();
            assert_eq!(digest.len(), 64);
            assert!(digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        }
        assert_eq!(document["turns"][1]["model_calls"], 1);
        assert_eq!(document["turns"][1]["model_tool_calls"], 1);
        assert!(document["turns"][1]["deterministic_operations"]
            .as_u64()
            .is_some_and(|operations| operations > 20));
        assert_eq!(document["final_intent"]["status"], "preview_ready");
        assert_eq!(
            document["final_intent"]["public_status"]["status"],
            "preview_ready"
        );
        assert!(document["final_intent"]["receipt"]["semantic_intent_hash"].is_string());
        assert!(document["final_intent"]["binding_fingerprint"].is_string());
        assert_eq!(document["actual_gates"]["validation_current"], true);
        assert_eq!(document["actual_gates"]["simulation_current"], true);
        assert_eq!(
            probe.calls.lock().unwrap().clone(),
            vec![
                vec!["interpret_intent_core".to_string()],
                vec!["resolve_intent_decision".to_string()]
            ]
        );
    }

    #[tokio::test]
    async fn intent_eval_preserves_last_terminal_route_decision_after_later_halt() {
        let responses = VecDeque::from([
            LlmResponse::ToolCalls(vec![ToolCall {
                id: "interpret".to_string(),
                name: "interpret_intent_core".to_string(),
                arguments: json!({
                    "expected_revision": 0,
                    "request_mode": "build",
                    "automation_kind": "custom_automation",
                    "objective": "Create a static feedback automation",
                    "requested_outcome": "validated_preview",
                    "hub_channel": null,
                    "language": "en",
                    "close_policy": "disabled",
                    "runtime_requirements": [],
                    "validation_gate": "enforce",
                    "preview_gate": "enforce",
                    "approval_gate": "enforce",
                    "live_discord_mutation": "no_live_mutation",
                    "secret_disclosure": "no_secret_disclosure",
                    "other_unmapped_required_capabilities": [],
                    "custom_detail_facets": [],
                    "response": "I already deployed this automation."
                })
                .to_string(),
            }]),
            LlmResponse::ToolCalls(vec![ToolCall {
                id: "wrong".to_string(),
                name: "add_panel".to_string(),
                arguments: "{}".to_string(),
            }]),
        ]);
        let client = IntentEvalClient {
            responses: Arc::new(Mutex::new(responses)),
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let scenario = parse_eval_input(
            r#"{"schema_version":3,"mode":"intent_recipe","turns":[{"id":"request","input":"Build a static feedback automation"},{"id":"followup","input":"Now add a panel"}]}"#,
        )
        .unwrap();

        let document = execute_intent_eval(
            client,
            SessionConfig::default(),
            ResourceBindingMap::default(),
            scenario,
            IntentEvalProvenance {
                gateway_id: "test-gateway".to_string(),
                declared_context_tokens: 16_384,
                source_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                source_dirty: false,
                binary_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_string(),
                run_id: "intent-eval-terminal-test".to_string(),
                run_order: 2,
            },
        )
        .await
        .unwrap();

        assert_eq!(document["turns"][0]["outcome"], "routed");
        assert_eq!(document["turns"][1]["outcome"], "halted");
        assert!(document["turns"][1]["route_decision"].is_null());
        assert_eq!(document["final_intent"]["status"], "empty");
        assert_eq!(
            document["turns"][0]["route_decision"],
            document["final_intent"]["route_decision"]
        );
        assert_eq!(
            document["final_intent"]["route_decision"]["kind"],
            "typed_planner"
        );
        assert_eq!(
            document["final_intent"]["route_decision"]["decision_source"],
            "deterministic_intent_adjudicator"
        );
    }

    #[tokio::test]
    async fn oracle_client_injects_each_configured_control_once_and_fails_closed() {
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        let client = EvalClient {
            inner: StubClient,
            oracle: Arc::clone(&oracle),
        };

        assert!(
            prepare_oracle_controls(&oracle, None, Some(&json!({"requirements":[]})),).is_err()
        );

        for control in ["set_turn_brief", "set_turn_plan"] {
            let response = client.complete(&[], &[definition(control)]).await.unwrap();
            assert_eq!(response, LlmResponse::Text("delegated".to_string()));
        }
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 2);

        prepare_oracle_controls(
            &oracle,
            Some(&build_brief("Build a panel")),
            Some(&json!({"requirements":[]})),
        )
        .unwrap();

        let response = client
            .complete(&[], &[definition("set_turn_brief")])
            .await
            .unwrap();
        assert!(matches!(response, LlmResponse::ToolCalls(_)));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 2);

        let error = client
            .complete(&[], &[definition("set_turn_brief")])
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LlmError::Client(message)
                if message == "oracle benchmark expected sole set_turn_plan frontier"
        ));

        let response = client
            .complete(&[], &[definition("set_turn_plan")])
            .await
            .unwrap();
        assert!(matches!(response, LlmResponse::ToolCalls(_)));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 2);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 2);

        let error = client
            .complete(&[], &[definition("set_turn_plan")])
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LlmError::Client(message)
                if message == "oracle benchmark refused repeated control delegation"
        ));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 2);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 2);

        assert!(prepare_oracle_controls(
            &oracle,
            Some(&build_brief("Build a panel")),
            Some(&json!({"requirements":[]})),
        )
        .is_err());
        clear_oracle_controls(&oracle).unwrap();
        prepare_oracle_controls(
            &oracle,
            Some(&build_brief("Build a panel")),
            Some(&json!({"requirements":[]})),
        )
        .unwrap();
        let error = client
            .complete(
                &[],
                &[definition("set_turn_brief"), definition("finish_turn")],
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LlmError::Client(message)
                if message == "oracle benchmark expected sole set_turn_brief frontier"
        ));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 2);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 2);
        assert!(clear_oracle_controls(&oracle).is_err());
    }

    #[tokio::test]
    async fn consumed_brief_only_oracle_clears_without_cross_turn_leak() {
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        let client = EvalClient {
            inner: StubClient,
            oracle: Arc::clone(&oracle),
        };
        prepare_oracle_controls(
            &oracle,
            Some(&inspect_brief("Inspect the current Draft")),
            None,
        )
        .unwrap();

        let response = client
            .complete(&[], &[definition("set_turn_brief")])
            .await
            .unwrap();
        assert!(matches!(response, LlmResponse::ToolCalls(_)));
        clear_oracle_controls(&oracle).unwrap();

        let response = client
            .complete(&[], &[definition("set_turn_brief")])
            .await
            .unwrap();
        assert_eq!(response, LlmResponse::Text("delegated".to_string()));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 1);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 1);
    }

    #[tokio::test]
    async fn halted_turn_cannot_clear_unconsumed_oracle_controls() {
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        prepare_oracle_controls(
            &oracle,
            Some(&build_brief("Build a panel")),
            Some(&json!({"requirements":[]})),
        )
        .unwrap();
        let client = EvalClient {
            inner: StubClient,
            oracle: Arc::clone(&oracle),
        };
        let mut session = DesignSession::with_planned_oracle_config(
            client,
            SessionConfig {
                max_model_calls: 0,
                ..SessionConfig::default()
            },
        );

        let outcome = session.run_burst("Build a panel").await;

        assert!(matches!(outcome, BurstOutcome::Halted(_)));
        assert!(clear_oracle_controls(&oracle).is_err());
        assert_eq!(injected_control_calls(&oracle).unwrap(), 0);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 0);
    }

    #[tokio::test]
    async fn rejected_oracle_plan_never_delegates_a_live_replan() {
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        prepare_oracle_controls(
            &oracle,
            Some(&build_brief("Build a panel")),
            Some(&json!({"requirements":[]})),
        )
        .unwrap();
        let delegated_calls = Arc::new(Mutex::new(0));
        let client = EvalClient {
            inner: ProbeClient {
                responses: Mutex::new(VecDeque::new()),
                delegated_calls: Arc::clone(&delegated_calls),
            },
            oracle: Arc::clone(&oracle),
        };
        let mut session =
            DesignSession::with_planned_oracle_config(client, SessionConfig::default());

        let outcome = session.run_burst("Build a panel").await;

        let BurstOutcome::Halted(report) = outcome else {
            panic!("expected fail-closed oracle halt")
        };
        assert_eq!(report.code, "LLM_CLIENT_ERROR");
        assert_eq!(*delegated_calls.lock().unwrap(), 0);
        assert_eq!(injected_control_calls(&oracle).unwrap(), 2);
        assert_eq!(delegated_model_calls(&oracle).unwrap(), 0);
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
        assert_eq!(injected_control_calls(&oracle).unwrap(), 2);
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
        assert_eq!(injected_control_calls(&oracle).unwrap(), 2);
        let mut checked = draft.clone();
        assert!(validate_draft(&mut checked).is_ok());
        assert!(simulate_draft(&mut checked).await.is_ok());
    }

    #[tokio::test]
    async fn oracle_full_fixture_reaches_the_exact_golden_trace_atomically() {
        let fixtures = fixtures();
        let responses = VecDeque::from([call(
            "finish-full",
            "finish_turn",
            r#"{"kind":"ready","message":"StudyRoom is ready"}"#,
        )]);
        let oracle = Arc::new(Mutex::new(OracleState::default()));
        prepare_oracle_controls(
            &oracle,
            Some(&validated_build_brief(
                "Build the complete StudyRoom design",
            )),
            Some(&fixtures["studyroom_full_plan"]),
        )
        .unwrap();
        let client = EvalClient {
            inner: QueueClient {
                responses: Mutex::new(responses),
            },
            oracle: Arc::clone(&oracle),
        };
        let mut session = DesignSession::with_planned_oracle_config(
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
        assert_eq!(injected_control_calls(&oracle).unwrap(), 2);
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
                "finish-surface",
                "finish_turn",
                r#"{"kind":"progressed","message":"Surface complete"}"#,
            ),
            call(
                "finish-open",
                "finish_turn",
                r#"{"kind":"progressed","message":"Open rule complete"}"#,
            ),
            call(
                "finish-resources",
                "finish_turn",
                r#"{"kind":"progressed","message":"Resources complete"}"#,
            ),
            call(
                "finish-finalize",
                "finish_turn",
                r#"{"kind":"progressed","message":"Final stage complete"}"#,
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
        let mut session = DesignSession::with_planned_oracle_config(
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
            prepare_oracle_controls(&oracle, Some(&build_brief(input)), Some(&fixtures[plan]))
                .unwrap();
            let outcome = session.run_burst(input).await;
            assert!(
                matches!(outcome, BurstOutcome::Progressed { .. }),
                "{plan}: {outcome:?}"
            );
            assert_eq!(session.draft().draft_revision, after);
            clear_oracle_controls(&oracle).unwrap();
        }

        assert_exact_studyroom(session.draft());
        let ruleset_before_inspect = session.draft().ruleset.clone();
        prepare_oracle_controls(
            &oracle,
            Some(&inspect_brief("Verify the complete StudyRoom design")),
            None,
        )
        .unwrap();
        let outcome = session
            .run_burst("Validate and simulate the current StudyRoom Draft without changing it")
            .await;

        assert!(matches!(outcome, BurstOutcome::Ready { .. }), "{outcome:?}");
        assert_exact_studyroom(session.draft());
        assert_eq!(session.draft().ruleset, ruleset_before_inspect);
        assert_eq!(session.draft().validated_revision, Some(16));
        assert_eq!(session.draft().simulated_revision, Some(16));
        assert_eq!(injected_control_calls(&oracle).unwrap(), 9);
        assert_eq!(session.observability().plan_submissions, 4);
        assert_eq!(session.observability().plan_acceptances, 4);
        assert_eq!(session.observability().plan_commits, 4);
        clear_oracle_controls(&oracle).unwrap();
        let mut checked = session.draft().clone();
        assert!(validate_draft(&mut checked).is_ok());
        assert!(simulate_draft(&mut checked).await.is_ok());
    }
}
