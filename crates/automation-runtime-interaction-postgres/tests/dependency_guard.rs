use std::fs;
use std::path::Path;

fn regular_dependencies(manifest: &str) -> &str {
    manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest)
}

fn rust_sources(directory: &Path) -> Vec<String> {
    let mut sources = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<Vec<_>>();
    sources.sort_unstable();
    sources
}

#[test]
fn adapter_dependency_surface_is_narrow() {
    let manifest = include_str!("../Cargo.toml");
    let regular = regular_dependencies(manifest);
    for required in [
        "automation-instance",
        "automation-ruleset",
        "automation-ruleset-dispatch",
        "sqlx",
        "tokio",
    ] {
        assert!(regular.contains(required), "missing dependency: {required}");
    }
    for forbidden in [
        "ai-gateway",
        "authoring-application",
        "automation-instance-postgres",
        "automation-ruleset-postgres",
        "automation-runtime =",
        "automation-runtime-convergence-postgres",
        "automation-runtime-panel-postgres",
        "axum",
        "reqwest",
        "rusqlite",
        "twilight",
    ] {
        assert!(
            !regular.contains(forbidden),
            "forbidden dependency: {forbidden}"
        );
    }
}

#[test]
fn pure_capability_crates_do_not_depend_on_the_postgres_adapter() {
    for manifest in [
        include_str!("../../automation-instance/Cargo.toml"),
        include_str!("../../automation-ruleset/Cargo.toml"),
        include_str!("../../automation-ruleset-dispatch/Cargo.toml"),
        include_str!("../../automation-runtime/Cargo.toml"),
    ] {
        let regular = regular_dependencies(manifest);
        assert!(!regular.contains("automation-runtime-interaction-postgres"));
    }
}

#[test]
fn adapter_sources_contain_no_comments() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = rust_sources(&root.join("src"));
    sources.push(include_str!("../build.rs").to_string());
    for source in sources {
        for line in source.lines() {
            let trimmed = line.trim_start();
            assert!(!trimmed.starts_with("//"));
            assert!(!trimmed.starts_with("/*"));
            assert!(!trimmed.starts_with('*'));
        }
    }
}

#[test]
fn adapter_contract_uses_only_four_private_capabilities() {
    let contract = include_str!("../src/contract.rs");
    let capabilities = [
        "starring_runtime_interaction_database_readiness_v1",
        "starring_runtime_interaction_route_read_v1",
        "starring_runtime_interaction_pinned_read_v1",
        "starring_runtime_interaction_instance_register_v1",
    ];
    assert_eq!(contract.matches("pub(crate) const ").count(), 4);
    assert_eq!(contract.matches("SELECT ").count(), 4);
    for capability in capabilities {
        assert_eq!(
            contract.matches(capability).count(),
            1,
            "unexpected capability reference count: {capability}"
        );
    }
    for forbidden in [
        "automation_instances",
        "ruleset_versions",
        "runtime_deployments",
        "INSERT ",
        "UPDATE ",
        "DELETE ",
        "TRUNCATE ",
        "CREATE ",
        "ALTER ",
        "DROP ",
    ] {
        assert!(!contract.contains(forbidden), "raw SQL edge: {forbidden}");
    }
}

#[test]
fn adapter_contract_placeholder_shapes_are_exact() {
    let contract = include_str!("../src/contract.rs");
    let expected = [
        ("DATABASE_READINESS_QUERY", 0_usize),
        ("ROUTE_READ_QUERY", 2),
        ("PINNED_READ_QUERY", 2),
        ("INSTANCE_REGISTER_QUERY", 7),
    ];
    for (name, expected_maximum) in expected {
        let query = contract
            .split(&format!("const {name}"))
            .nth(1)
            .and_then(|tail| tail.split(';').next())
            .unwrap();
        let maximum = query
            .split('$')
            .skip(1)
            .filter_map(|part| {
                part.chars()
                    .take_while(|character| character.is_ascii_digit())
                    .collect::<String>()
                    .parse::<usize>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        assert_eq!(maximum, expected_maximum, "placeholder drift: {name}");
        for parameter in 1..=maximum {
            assert!(query.contains(&format!("${parameter}")));
        }
    }
}

#[test]
fn adapter_implements_only_the_narrow_interaction_traits() {
    let store = include_str!("../src/store.rs");
    for capability in [
        "impl InstanceRouteReaderV1 for PostgresRuntimeInteractionV1",
        "impl InstanceRegistrarV1 for PostgresRuntimeInteractionV1",
        "impl PinnedInstanceResolverV1 for PostgresRuntimeInteractionV1",
    ] {
        assert!(
            store.contains(capability),
            "missing capability: {capability}"
        );
    }
    for forbidden in [
        "impl InstanceStore for",
        "impl RuleSetStore for",
        "LegacyInstanceStoreCapabilitiesV1",
        "PostgresInstanceStore",
        "PostgresRuleSetStore",
    ] {
        assert!(!store.contains(forbidden), "broad capability: {forbidden}");
    }
}

#[test]
fn adapter_cannot_be_constructed_without_readiness_verification() {
    let store = include_str!("../src/store.rs");
    let constructor = store
        .find("pub async fn connect_verified_with_route_timeout(")
        .unwrap();
    let verification = store[constructor..]
        .find("verify_runtime_interaction_database_with_timeouts_v1")
        .unwrap();
    let construction = store[constructor..].find("Ok(Self {").unwrap();
    assert!(verification < construction);
    for forbidden in [
        "pub fn new(",
        "pub fn from_pool(",
        "pub fn pool(",
        "pub fn pool_mut(",
        "pub pool:",
    ] {
        assert!(
            !store.contains(forbidden),
            "unverified surface: {forbidden}"
        );
    }
}

#[test]
fn route_read_deadline_wraps_the_complete_database_operation() {
    let store = include_str!("../src/store.rs");
    let timeout = store.find("tokio::time::timeout_at(").unwrap();
    let operation = store[timeout..]
        .find("self.read_instance_route_operation_v1")
        .unwrap();
    let helper = store
        .find("async fn read_instance_route_operation_v1(")
        .unwrap();
    let begin = store[helper..]
        .find("begin_interaction_transaction_on_connection")
        .unwrap();
    let query = store[helper..].find("ROUTE_READ_QUERY").unwrap();
    let decode = store[helper..]
        .find(".decode(guild_id, instance_id)")
        .unwrap();
    let commit = store[helper..].find(".commit()").unwrap();
    assert!(operation > 0);
    assert!(begin < query && query < decode && decode < commit);
    assert!(store.contains("RouteConnectionGuardV1::new"));
    assert!(store.contains("InstanceStoreError::TimedOut"));

    let guard = include_str!("../src/route_connection.rs");
    assert!(guard.contains("impl Drop for RouteConnectionGuardV1"));
    assert!(guard.contains("drop(connection.detach())"));
    assert!(guard.contains("release_to_pool"));
}

#[test]
fn adapter_database_transactions_are_bounded_in_canonical_order() {
    let database = include_str!("../src/database.rs");
    let statement = database
        .find("pg_catalog.set_config('statement_timeout'")
        .unwrap();
    let lock = database
        .find("pg_catalog.set_config('lock_timeout'")
        .unwrap();
    let idle = database
        .find("pg_catalog.set_config('idle_in_transaction_session_timeout'")
        .unwrap();
    let search_path = database
        .find("pg_catalog.set_config('search_path'")
        .unwrap();
    assert!(statement < lock && lock < idle && idle < search_path);
    assert!(database.contains("MAX_RUNTIME_INTERACTION_DATABASE_TIMEOUT"));
    assert!(database.contains("begin_interaction_transaction"));
    assert!(database.contains("lock_timeout >= statement_timeout"));
}

#[test]
fn persisted_rows_are_revalidated_before_dispatch() {
    let row = include_str!("../src/row.rs");
    for invariant in [
        "guild_id != expected_guild_id",
        "&instance_id != expected_instance_id",
        "CURRENT_RULESET_SCHEMA_VERSION",
        "automation_core::validate_structural",
        "content_hash(schema_version, &definition)",
        "persisted_hash != recomputed_hash",
        "InstanceStatus::Active",
    ] {
        assert!(
            row.contains(invariant),
            "missing row invariant: {invariant}"
        );
    }
    let inactive = row
        .find("instance.status != InstanceStatus::Active")
        .unwrap();
    let artifact = row.find("let artifact_fields = [").unwrap();
    assert!(inactive < artifact);
}

#[test]
fn database_failures_are_redacted_to_stable_codes() {
    let error = include_str!("../src/error.rs");
    let store = include_str!("../src/store.rs");
    for code in [
        "runtime_interaction_invalid_input",
        "runtime_interaction_invalid_authority",
        "runtime_interaction_conflict",
        "runtime_interaction_persistence_corrupt",
        "runtime_interaction_timeout",
        "runtime_interaction_unavailable",
        "runtime_interaction_indeterminate",
    ] {
        assert_eq!(error.matches(code).count(), 1, "stable code drift: {code}");
    }
    for leak in [
        "error.to_string()",
        "format!(\"{error}",
        "format!(\"{error:?}",
        "database.message()",
        "database.detail()",
    ] {
        assert!(!store.contains(leak), "database detail leak: {leak}");
    }
}
