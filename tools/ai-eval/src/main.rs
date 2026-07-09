fn main() {
    let fixtures = ai_eval::fixtures();

    #[cfg(feature = "openai-client")]
    {
        match ai_gateway::OpenAiCompatibleClient::from_env() {
            Ok(client) => {
                let model = client.model().to_string();
                let report = ai_eval::evaluate(&client, &model, &fixtures);
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                if let Err(error) = write_artifacts(timestamp, &report) {
                    eprintln!("artifact write failed: {error}");
                }
                print!("{}", report.render());
                return;
            }
            Err(error) => {
                eprintln!("no endpoint: {error}");
                std::process::exit(1);
            }
        }
    }

    #[cfg(not(feature = "openai-client"))]
    {
        let _ = fixtures;
        eprintln!("ai-eval: rebuild with --features openai-client and set AI_BASE_URL/AI_MODEL to run against a model.");
        std::process::exit(1);
    }
}

#[cfg(feature = "openai-client")]
fn write_artifacts(timestamp: u64, report: &ai_eval::EvaluationReport) -> std::io::Result<()> {
    let base = format!("ai-eval-runs/{timestamp}");
    let mut summaries = Vec::new();
    for result in &report.results {
        let dir = format!("{base}/{}", result.name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(format!("{dir}/raw.txt"), &result.raw)?;
        if let Some(desired_json) = &result.desired_json {
            std::fs::write(format!("{dir}/desired.json"), desired_json)?;
        }
        summaries.push(serde_json::json!({
            "name": result.name,
            "reached": format!("{:?}", result.reached),
            "safe": result.safety_violations.is_empty(),
            "safety_violations": result.safety_violations,
            "failure": result.failure,
        }));
    }
    std::fs::write(
        format!("{base}/report.json"),
        serde_json::to_string_pretty(&summaries).unwrap_or_default(),
    )?;
    Ok(())
}

#[cfg(all(test, feature = "openai-client"))]
mod tests {
    use super::write_artifacts;
    use ai_eval::{EvalStage, EvaluationReport, FixtureResult};

    #[test]
    fn writes_artifacts() {
        let timestamp = 9876543210;
        let report = EvaluationReport {
            results: vec![FixtureResult {
                name: "fixture".to_string(),
                reached: EvalStage::Graphed,
                failure: None,
                raw: "raw".to_string(),
                desired_json: Some("{\"mode\":\"patch\"}".to_string()),
                safety_violations: vec![],
            }],
        };
        let base = format!("ai-eval-runs/{timestamp}");
        let _ = std::fs::remove_dir_all(&base);
        write_artifacts(timestamp, &report).unwrap();
        assert_eq!(
            std::fs::read_to_string(format!("{base}/fixture/raw.txt")).unwrap(),
            "raw"
        );
        assert_eq!(
            std::fs::read_to_string(format!("{base}/fixture/desired.json")).unwrap(),
            "{\"mode\":\"patch\"}"
        );
        assert!(std::fs::read_to_string(format!("{base}/report.json"))
            .unwrap()
            .contains("\"fixture\""));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
