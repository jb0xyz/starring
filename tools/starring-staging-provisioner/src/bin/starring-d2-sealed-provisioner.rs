use std::path::PathBuf;
use std::process::ExitCode;

use starring_staging_provisioner::{
    onboard_d2_from_manifest, provision_d2_from_manifest, quarantine_d2_from_manifest,
    D2ProvisionerErrorV1,
};

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(mode) = arguments.first().and_then(|value| value.to_str()) else {
        eprintln!("{}", D2ProvisionerErrorV1::Arguments.code());
        return ExitCode::from(64);
    };
    match mode {
        "provision" | "quarantine" => {
            let [_, manifest_flag, manifest_path] = arguments.as_slice() else {
                eprintln!("{}", D2ProvisionerErrorV1::Arguments.code());
                return ExitCode::from(64);
            };
            if manifest_flag != "--manifest" {
                eprintln!("{}", D2ProvisionerErrorV1::Arguments.code());
                return ExitCode::from(64);
            }
            let manifest_path = PathBuf::from(manifest_path);
            if mode == "provision" {
                provision(&manifest_path).await
            } else {
                quarantine(&manifest_path).await
            }
        }
        "onboard" => {
            let [_, manifest_flag, manifest_path, principal_flag, principal_id, display_flag, display_name, installation_flag, installation_id] =
                arguments.as_slice()
            else {
                eprintln!("{}", D2ProvisionerErrorV1::Arguments.code());
                return ExitCode::from(64);
            };
            let (Some(principal_id), Some(display_name), Some(installation_id)) = (
                principal_id.to_str(),
                display_name.to_str(),
                installation_id.to_str(),
            ) else {
                eprintln!("{}", D2ProvisionerErrorV1::Arguments.code());
                return ExitCode::from(64);
            };
            if manifest_flag != "--manifest"
                || principal_flag != "--principal-id"
                || display_flag != "--display-name"
                || installation_flag != "--installation-id"
            {
                eprintln!("{}", D2ProvisionerErrorV1::Arguments.code());
                return ExitCode::from(64);
            }
            match onboard_d2_from_manifest(
                &PathBuf::from(manifest_path),
                principal_id,
                display_name,
                installation_id,
            )
            .await
            {
                Ok(report) => {
                    println!(
                        "{{\"outcome\":\"{}\",\"installation_id\":\"{}\",\"principal_id\":\"{}\"}}",
                        report.outcome().as_str(),
                        report.installation_id(),
                        report.principal_id(),
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{}", error.code());
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("{}", D2ProvisionerErrorV1::Arguments.code());
            ExitCode::from(64)
        }
    }
}

async fn provision(manifest_path: &std::path::Path) -> ExitCode {
    match provision_d2_from_manifest(manifest_path).await {
        Ok(report) => {
            println!(
                    "{{\"outcome\":\"{}\",\"application_credentials\":{},\"keyrings\":{},\"worker_credentials\":{},\"external_credentials_checked\":{},\"activated_roles\":{}}}",
                    report.outcome().as_str(),
                    report.application_credentials(),
                    report.keyrings(),
                    report.worker_credentials(),
                    report.external_credentials_checked(),
                    report.activated_roles(),
                );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.code());
            ExitCode::FAILURE
        }
    }
}

async fn quarantine(manifest_path: &std::path::Path) -> ExitCode {
    match quarantine_d2_from_manifest(manifest_path).await {
        Ok(report) => {
            println!(
                    "{{\"outcome\":\"quarantined\",\"quarantined_roles\":{},\"removed_credentials\":{}}}",
                    report.quarantined_roles(),
                    report.removed_credentials(),
                );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.code());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn candidate_source_has_only_nonsecret_arguments_and_redacted_output() {
        let source = include_str!("starring-d2-sealed-provisioner.rs");
        assert!(source.contains("provision"));
        assert!(source.contains("quarantine"));
        assert!(source.contains("--manifest"));
        assert!(!source.contains(&["--", "password"].concat()));
        assert!(!source.contains(&["--", "token"].concat()));
        assert!(!source.contains(&["key", "_id"].concat()));
    }
}
