use std::process::ExitCode;

use starring_staging_provisioner::{
    postgres_environment_is_present, provision_authoring_writer,
    provision_interaction_token_keyring, provision_staging, verify_final, ProvisionerErrorV1,
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
    let (mode, system_identifier, acknowledgement) = match arguments.as_slice() {
        [system_identifier, acknowledgement] => ("provision", *system_identifier, *acknowledgement),
        [mode, system_identifier, acknowledgement] if *mode == "--verify-final" => {
            ("verify", *system_identifier, *acknowledgement)
        }
        [mode, system_identifier, acknowledgement] if *mode == "--provision-authoring-writer" => {
            ("authoring-writer", *system_identifier, *acknowledgement)
        }
        [mode, system_identifier, acknowledgement]
            if *mode == "--provision-interaction-token-keyring" =>
        {
            (
                "interaction-token-keyring",
                *system_identifier,
                *acknowledgement,
            )
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
    if mode == "interaction-token-keyring" {
        match provision_interaction_token_keyring(acknowledgement) {
            Ok(report) => {
                println!(
                    "outcome={} active_key_id={}",
                    report.outcome().as_str(),
                    report.active_key_id()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{}", error.code());
                ExitCode::from(1)
            }
        }
    } else if mode == "verify" {
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
    } else if mode == "authoring-writer" {
        match provision_authoring_writer(acknowledgement).await {
            Ok(report) => {
                println!(
                    "provisioned authoring_writer={} database={} credential_items=1 capabilities=5 snapshot_reader=v2_only",
                    report.outcome().as_str(),
                    report.database()
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
                    "provisioned database=starring_runtime_staging application_database_credentials=20 keyrings=3 product_action_key_id={} snapshot_envelope_key_id={} interaction_token_envelope_key_id={}",
                    report.product_action_key_id(),
                    report.snapshot_envelope_key_id(),
                    report.interaction_token_envelope_key_id()
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
