#[test]
fn regular_dependencies_stay_transport_and_storage_neutral() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap_or("")
        .split("[dev-dependencies]")
        .next()
        .unwrap_or("");
    for forbidden in [
        "tokio",
        "twilight",
        "sqlx",
        "rusqlite",
        "sqlite",
        "reqwest",
        "ai-gateway",
        "ai_gateway",
        "ollama",
        "llm",
        "design-harness",
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
        ("src/error.rs", include_str!("../src/error.rs")),
        ("src/identity.rs", include_str!("../src/identity.rs")),
        ("src/lib.rs", include_str!("../src/lib.rs")),
        ("src/registry.rs", include_str!("../src/registry.rs")),
        (
            "tests/dependency_guard.rs",
            include_str!("dependency_guard.rs"),
        ),
        ("tests/registry.rs", include_str!("registry.rs")),
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
