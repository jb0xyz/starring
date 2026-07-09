fn main() {
    let fixtures = ai_eval::fixtures();

    #[cfg(feature = "openai-client")]
    {
        match ai_gateway::OpenAiCompatibleClient::from_env() {
            Ok(client) => {
                let model = client.model().to_string();
                let report = ai_eval::evaluate(&client, &model, &fixtures);
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
