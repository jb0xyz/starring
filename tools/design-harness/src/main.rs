mod client;
mod config;
mod eval;
mod store;

use std::env;
use std::error::Error;
use std::time::Instant;

use design_harness::{
    BurstOutcome, DesignSession, DraftSummary, HaltReport, LimitKind, Observability,
    StructuredError,
};
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
        None => DesignSession::with_config(client, config.session_config),
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
            BurstOutcome::AwaitingHuman { question } => {
                write_line(&mut output, &format!("assistant> {question}")).await?;
            }
            BurstOutcome::Completed { summary } => {
                write_line(&mut output, &format!("completed> {summary}")).await?;
                write_draft(&mut output, &session.draft().summary()).await?;
                write_observability(&mut output, session.observability()).await?;
                break;
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
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt).await?;
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("evaluation prompt must not be empty".into());
    }
    let mut session = DesignSession::with_config(client, config);
    let started = Instant::now();
    let outcome = session.run_burst(prompt).await;
    let document = eval::report(&session, &outcome, started.elapsed()).await;
    let mut output = io::stdout();
    output
        .write_all(serde_json::to_string(&document)?.as_bytes())
        .await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
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
            "metrics> model_calls={} tool_calls={} mutation_tools={} clarifications={} validation_failures={} simulation_failures={} repeated_errors={} repair_attempts={} repair_successes={} repair_failures={} repair_escalations={} nudges={}",
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
            observability.nudge_count
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
