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
        "tracing",
        "log",
        "sentry",
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
        ("src/oauth.rs", include_str!("../src/oauth.rs")),
        ("src/oauth_tests.rs", include_str!("../src/oauth_tests.rs")),
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
                    && !trimmed.ends_with("*/"),
                "source comment at {path}:{}",
                index + 1
            );
        }
    }
}

#[test]
fn mutation_authorities_have_distinct_length_framed_digest_domains() {
    let adapter = include_str!("../src/adapter.rs");
    let base = "starring.discord-authority.v1";
    let author = "starring.discord-authority.author.v1";
    let apply = "starring.discord-authority.apply-runtime.v1";
    let cancellation = "starring.discord-authority.cancel-lifecycle-runtime.v1";
    let domains = [base, author, apply, cancellation];
    for (index, left) in domains.iter().enumerate() {
        for right in domains.iter().skip(index + 1) {
            assert_ne!(left, right);
        }
    }
    for domain in domains {
        assert_eq!(adapter.matches(domain).count(), 1);
    }
    assert!(adapter.contains("update_field(&mut hasher, digest_domain);"));
    assert!(adapter.contains("CapabilityV1::Author => AUTHOR_AUTHORITY_DIGEST_DOMAIN_V1"));
    assert!(adapter.contains(
        "CapabilityV1::CancelLifecycle => CANCEL_LIFECYCLE_RUNTIME_AUTHORITY_DIGEST_DOMAIN_V1"
    ));
    let compact = adapter.split_whitespace().collect::<String>();
    assert!(compact.contains("CapabilityV1::Author=>b\"author\""));
    assert!(compact.contains(
        "CapabilityV1::Author|CapabilityV1::Promote|CapabilityV1::Approve|CapabilityV1::Reject|CapabilityV1::Apply|CapabilityV1::CancelLifecycle=>self.write_lifetime"
    ));
    assert!(compact.contains("ifwrite_lifetime.is_zero()||write_lifetime>Duration::from_secs(5)"));
    assert!(compact.contains(
        "Self::new(Duration::from_secs(5),Duration::from_secs(5),Duration::from_secs(30),)"
    ));
    assert!(
        compact.contains("matches!(capability,CapabilityV1::Apply|CapabilityV1::CancelLifecycle)")
    );
    assert!(!compact.contains("matches!(capability,CapabilityV1::Author|CapabilityV1::Apply"));
}
