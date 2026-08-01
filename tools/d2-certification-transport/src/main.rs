use d2_certification_transport::{run, Config};

#[tokio::main]
async fn main() {
    let config = match Config::from_process_arguments() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{}", error.code());
            std::process::exit(64);
        }
    };
    if let Err(error) = run(config).await {
        eprintln!("{error}");
        std::process::exit(70);
    }
}
