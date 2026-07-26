use std::process::Command;

fn referenced_command() -> Command {
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

fn configured_command() -> Command {
    let mut command = referenced_command();
    for (index, capability) in [
        "convergence",
        "exact_target",
        "panel",
        "serving",
        "interaction",
    ]
    .into_iter()
    .enumerate()
    {
        command.env(
            format!("STARRING_RUNTIME_SECRET_DATABASE_{index}"),
            database_url(capability),
        );
    }
    command.env(
        "STARRING_RUNTIME_SECRET_DISCORD_BOT_TOKEN",
        "opaque.discord_bot-token_1234567890abcdef",
    );
    command
}

#[test]
fn valid_runtime_configuration_stays_at_an_exact_closed_boundary() {
    let output = configured_command().output().unwrap();
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    if std::env::var_os("STARRING_RUNTIME_TEST_REQUIRE_COMPILED_REVISION").is_some() {
        assert_eq!(output.status.code(), Some(69));
        assert_eq!(
            stderr,
            "starring_runtime_status=runtime_database_unavailable context=convergence\n"
        );
    } else {
        assert!(
            matches!(
                (output.status.code(), stderr.as_str()),
                (
                    Some(69),
                    "starring_runtime_status=runtime_database_unavailable context=convergence\n"
                ) | (
                    Some(78),
                    "starring_runtime_status=runtime_build_revision_missing\n"
                )
            ),
            "runtime did not stop at an exact closed boundary"
        );
    }
    for forbidden in [
        database_password(),
        "opaque.discord_bot-token_1234567890abcdef".to_string(),
        database_socket_path(),
        "ready".to_string(),
        "owner".to_string(),
        "discord".to_string(),
    ] {
        assert!(!stderr
            .to_ascii_lowercase()
            .contains(&forbidden.to_ascii_lowercase()));
    }
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

#[test]
fn missing_secret_exits_with_stable_capability_context() {
    let output = referenced_command().output().unwrap();
    assert_eq!(output.status.code(), Some(78));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "starring_runtime_status=runtime_secret_missing context=convergence\n"
    );
}

#[test]
fn invalid_secret_output_never_contains_the_secret() {
    let marker = "runtime-secret-marker";
    let output = referenced_command()
        .env("STARRING_RUNTIME_SECRET_DATABASE_0", marker)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(78));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr,
        "starring_runtime_status=runtime_secret_invalid context=convergence\n"
    );
    assert!(!stderr.contains(marker));
}

#[test]
fn duplicate_database_identity_exits_with_stable_capability_context() {
    let duplicate = database_url("convergence");
    let output = configured_command()
        .env("STARRING_RUNTIME_SECRET_DATABASE_1", &duplicate)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(78));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr,
        "starring_runtime_status=runtime_secret_database_identity_duplicate context=exact_target\n"
    );
    assert!(!stderr.contains(&duplicate));
}

fn database_url(capability: &str) -> String {
    format!(
        "postgresql:{}{}runtime_{capability}:{}@localhost:5432/starring?host={}&sslmode=disable",
        "/",
        "/",
        database_password(),
        database_socket_path()
    )
}

fn database_password() -> String {
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef0123456789_-".to_string()
}

fn database_socket_path() -> String {
    format!("/tmp/starring_runtime_no_postgres_{}", std::process::id())
}
