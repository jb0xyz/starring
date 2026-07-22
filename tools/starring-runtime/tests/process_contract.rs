use std::process::Command;

fn configured_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_starring-runtime"));
    command.env_clear();
    for (name, value) in [
        (
            "STARRING_RUNTIME_CONVERGENCE_DATABASE_URL_SECRET_REFERENCE",
            "env:STARRING_RUNTIME_SECRET_DATABASE_0",
        ),
        (
            "STARRING_RUNTIME_EXACT_TARGET_DATABASE_URL_SECRET_REFERENCE",
            "env:STARRING_RUNTIME_SECRET_DATABASE_1",
        ),
        (
            "STARRING_RUNTIME_PANEL_DATABASE_URL_SECRET_REFERENCE",
            "env:STARRING_RUNTIME_SECRET_DATABASE_2",
        ),
        (
            "STARRING_RUNTIME_SERVING_DATABASE_URL_SECRET_REFERENCE",
            "env:STARRING_RUNTIME_SECRET_DATABASE_3",
        ),
        (
            "STARRING_RUNTIME_INTERACTION_DATABASE_URL_SECRET_REFERENCE",
            "env:STARRING_RUNTIME_SECRET_DATABASE_4",
        ),
        (
            "STARRING_RUNTIME_DISCORD_BOT_TOKEN_SECRET_REFERENCE",
            "env:STARRING_RUNTIME_SECRET_DISCORD_BOT_TOKEN",
        ),
    ] {
        command.env(name, value);
    }
    command
}

#[test]
fn valid_configuration_exits_as_not_composed_without_claiming_readiness() {
    let output = configured_command().output().unwrap();
    assert_eq!(output.status.code(), Some(70));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "starring_runtime_status=runtime_not_composed\n"
    );
}

#[test]
fn invalid_configuration_exits_with_stable_redacted_context() {
    let output = Command::new(env!("CARGO_BIN_EXE_starring-runtime"))
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(78));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "starring_runtime_status=runtime_config_missing context=convergence_database_url_secret_reference\n"
    );
}
