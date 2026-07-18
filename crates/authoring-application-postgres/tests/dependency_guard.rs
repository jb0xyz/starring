#[test]
fn pure_application_does_not_depend_on_postgres_adapter() {
    let manifest = include_str!("../../authoring-application/Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!regular.contains("authoring-application-postgres"));
    assert!(!regular.contains("sqlx"));
}

#[test]
fn adapter_stays_out_of_http_discord_and_decision_mutations() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    for forbidden in [
        "axum",
        "reqwest",
        "twilight",
        "authoring-application-discord",
        "automation-ruleset-activation-postgres",
        "authoring-promotion-postgres",
    ] {
        assert!(
            !regular.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
    let source = include_str!("../src/lib.rs");
    assert!(!source.contains("ProductDecisionPort"));
}

#[test]
fn authentication_and_snapshot_transactions_keep_their_security_shape() {
    let authentication = include_str!("../src/authentication.rs");
    assert!(authentication.contains("digest_opaque_session_credential_v1(credential)"));
    assert!(authentication.contains("FOR UPDATE OF authentication_session, principal"));
    assert!(!authentication.contains(".bind(credential)"));
    let snapshot = include_str!("../src/snapshot.rs");
    assert!(snapshot.contains("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY"));
    assert!(snapshot.contains("CURRENT_TIMESTAMP AS database_now"));
    assert!(snapshot.contains("evidence.installation_authority_revision().get()"));
    assert!(snapshot.contains("evidence.installation_authority_digest()"));
}

#[test]
fn source_files_contain_no_comments() {
    let sources = [
        (
            "src/authentication.rs",
            include_str!("../src/authentication.rs"),
        ),
        ("src/bindings.rs", include_str!("../src/bindings.rs")),
        ("src/digest.rs", include_str!("../src/digest.rs")),
        ("src/envelope.rs", include_str!("../src/envelope.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/snapshot.rs", include_str!("../src/snapshot.rs")),
        (
            "tests/dependency_guard.rs",
            include_str!("dependency_guard.rs"),
        ),
        (
            "tests/postgres_adapter.rs",
            include_str!("postgres_adapter.rs"),
        ),
    ];
    for (path, source) in sources {
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            assert!(
                !trimmed.starts_with("//")
                    && !trimmed.starts_with("/*")
                    && !trimmed.starts_with('*')
                    && !trimmed.ends_with("*/"),
                "source comment at {path}:{}",
                index + 1
            );
        }
    }
}
