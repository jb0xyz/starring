#[test]
fn regular_dependencies_stay_pure() {
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
        "sqlite",
        "twilight",
        "ai-gateway",
        "ai_gateway",
        "ollama",
        "llm",
        "reqwest",
        "design-harness",
        "automation-runtime =",
        "automation-runtime-convergence-postgres",
        "tokio",
    ] {
        assert!(
            !regular.contains(forbidden),
            "forbidden regular dependency: {forbidden}"
        );
    }
}

#[test]
fn crate_is_library_only() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("[[bin]]"));
}

#[test]
fn source_files_contain_no_comments() {
    let sources = [
        ("src/config.rs", include_str!("../src/config.rs")),
        ("src/dto.rs", include_str!("../src/dto.rs")),
        ("src/failure.rs", include_str!("../src/failure.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/planner.rs", include_str!("../src/planner.rs")),
        ("src/port.rs", include_str!("../src/port.rs")),
        ("src/retry.rs", include_str!("../src/retry.rs")),
        ("src/session.rs", include_str!("../src/session.rs")),
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
