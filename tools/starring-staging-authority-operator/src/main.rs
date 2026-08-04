use std::process::ExitCode;

use starring_staging_authority_operator::{
    advance_authority, postgres_environment_is_present, AuthorityAdvanceCommandV1,
    AuthorityAdvanceCommandValuesV1,
};

#[tokio::main]
async fn main() -> ExitCode {
    if postgres_environment_is_present() {
        eprintln!("postgres_environment_not_allowed");
        return ExitCode::from(64);
    }
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(arguments) = arguments
        .iter()
        .map(|argument| argument.to_str())
        .collect::<Option<Vec<_>>>()
    else {
        eprintln!("command_line_arguments_not_allowed");
        return ExitCode::from(64);
    };
    let [system_identifier, installation_id, channel_id, acknowledgement] = arguments.as_slice()
    else {
        eprintln!("command_line_arguments_not_allowed");
        return ExitCode::from(64);
    };
    let command = match AuthorityAdvanceCommandV1::parse(AuthorityAdvanceCommandValuesV1 {
        system_identifier: (*system_identifier).to_string(),
        installation_id: (*installation_id).to_string(),
        channel_id: (*channel_id).to_string(),
        acknowledgement: (*acknowledgement).to_string(),
    }) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{}", error.code());
            return ExitCode::from(64);
        }
    };
    match advance_authority(&command).await {
        Ok(report) => {
            println!(
                "result={} installation_id={} authority_revision={} binding_key={} channel_id={}",
                report.outcome().as_str(),
                report.installation_id(),
                report.authority_revision(),
                report.binding_key(),
                report.channel_id()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.code());
            ExitCode::from(1)
        }
    }
}
