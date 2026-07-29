use std::process::ExitCode;

use starring_staging_provisioner::{
    postgres_environment_is_present, provision_staging, verify_final, ProvisionerErrorV1,
    StagingAcknowledgementV1,
};

#[tokio::main]
async fn main() -> ExitCode {
    if postgres_environment_is_present() {
        eprintln!("{}", ProvisionerErrorV1::PostgresEnvironment.code());
        return ExitCode::from(64);
    }
    let raw_arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(arguments) = raw_arguments
        .iter()
        .map(|argument| argument.to_str())
        .collect::<Option<Vec<_>>>()
    else {
        eprintln!("{}", ProvisionerErrorV1::CommandLineArguments.code());
        return ExitCode::from(64);
    };
    let (verify, system_identifier, acknowledgement) = match arguments.as_slice() {
        [system_identifier, acknowledgement] => (false, *system_identifier, *acknowledgement),
        [mode, system_identifier, acknowledgement] if *mode == "--verify-final" => {
            (true, *system_identifier, *acknowledgement)
        }
        _ => {
            eprintln!("{}", ProvisionerErrorV1::CommandLineArguments.code());
            return ExitCode::from(64);
        }
    };
    let acknowledgement = match StagingAcknowledgementV1::parse(system_identifier, acknowledgement)
    {
        Ok(acknowledgement) => acknowledgement,
        Err(error) => {
            eprintln!("{}", error.code());
            return ExitCode::from(64);
        }
    };
    if verify {
        match verify_final(acknowledgement).await {
            Ok(report) => {
                println!(
                    "verified database={} application_database_credentials={} keyrings={} hba_rules={}",
                    report.database(),
                    report.application_database_credentials(),
                    report.keyrings(),
                    report.hba_rules()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{}", error.code());
                ExitCode::from(1)
            }
        }
    } else {
        match provision_staging(acknowledgement).await {
            Ok(report) => {
                println!(
                    "provisioned database=starring_runtime_staging application_database_credentials=20 keyrings=2 product_action_key_id={} snapshot_envelope_key_id={}",
                    report.product_action_key_id(),
                    report.snapshot_envelope_key_id()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{}", error.code());
                ExitCode::from(1)
            }
        }
    }
}
