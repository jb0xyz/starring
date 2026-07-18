#[test]
fn adapter_excludes_database_http_edge_and_model_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or("")
        .split("[dev-dependencies]")
        .next()
        .unwrap_or("");
    for forbidden in [
        "sqlx",
        "rusqlite",
        "axum",
        "design-harness",
        "ai-gateway",
        "ollama",
        "llm",
        "automation-ruleset-activation",
        "authoring-promotion-postgres",
    ] {
        assert!(
            !regular.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
}

#[test]
fn source_files_contain_no_comments() {
    let sources = [
        ("src/adapter.rs", include_str!("../src/adapter.rs")),
        ("src/evidence.rs", include_str!("../src/evidence.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/snapshot.rs", include_str!("../src/snapshot.rs")),
        ("src/twilight.rs", include_str!("../src/twilight.rs")),
        ("tests/authority.rs", include_str!("authority.rs")),
        (
            "tests/dependency_guard.rs",
            include_str!("dependency_guard.rs"),
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
