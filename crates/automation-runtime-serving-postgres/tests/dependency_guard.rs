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
    for required in ["automation-runtime-controller", "sqlx", "tokio"] {
        assert!(regular.contains(required), "missing dependency: {required}");
    }
    for forbidden in [
        "automation-runtime =",
        "automation-runtime-convergence-postgres",
        "automation-runtime-interaction-postgres",
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
fn pure_controller_does_not_depend_on_the_postgres_adapter() {
    let manifest = include_str!("../../automation-runtime-controller/Cargo.toml");
    assert!(!regular_dependencies(manifest).contains("automation-runtime-serving-postgres"));
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
            assert!(!trimmed.ends_with("*/"));
        }
    }
}

#[test]
fn adapter_contract_uses_only_private_serving_capabilities() {
    let contract = include_str!("../src/contract.rs");
    for capability in [
        "starring_runtime_serving_database_readiness_v1",
        "starring_runtime_serving_database_identity_v1",
        "starring_runtime_serving_heartbeat_v1",
        "starring_runtime_serving_disconnect_v1",
    ] {
        assert!(contract.contains(capability));
    }
    for forbidden in [
        "runtime_deployments",
        "runtime_serving_leases",
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
        ("DATABASE_READINESS_DEFINITION_QUERY", 0),
        ("DATABASE_BINDING_QUERY", 0),
        ("HEARTBEAT_QUERY", 9),
        ("DISCONNECT_QUERY", 8),
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
fn adapter_implements_only_the_serving_lease_port() {
    let store = include_str!("../src/store.rs");
    assert!(store.contains("impl RuntimeServingLeasePort for PostgresRuntimeServingLeaseV1"));
    for forbidden in [
        "RuntimeExecutionConvergencePort",
        "RuntimePreviousServingObservationPort",
        "PostgresRuntimeConvergence",
        "claim_next_execution",
        "certify_live",
    ] {
        assert!(!store.contains(forbidden), "broad capability: {forbidden}");
    }
}

#[test]
fn adapter_cannot_be_constructed_without_readiness_verification() {
    let store = include_str!("../src/store.rs");
    let constructor = store.find("pub async fn connect_verified(").unwrap();
    let verification = store[constructor..]
        .find("verify_runtime_serving_database_with_timeouts_v1")
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
fn mutations_use_absolute_deadlines_and_cancellation_fencing() {
    let store = include_str!("../src/store.rs");
    let deadline = store
        .find("let deadline = tokio::time::Instant::now()")
        .unwrap();
    let acquire = store.find("self.pool.acquire()").unwrap();
    let operation = store.find("self.execute_mutation_on_connection").unwrap();
    let helper = store
        .find("async fn execute_mutation_on_connection(")
        .unwrap();
    let operation_body = &store[helper..];
    let begin = operation_body
        .find("begin_serving_mutation_transaction")
        .unwrap();
    let binding = operation_body
        .find("verify_runtime_serving_binding_v1")
        .unwrap();
    let query = operation_body.find("HEARTBEAT_QUERY").unwrap();
    let decode = operation_body
        .find("row.decode_heartbeat(request)")
        .unwrap();
    let commit = operation_body.find(".commit()").unwrap();
    assert!(deadline < acquire && acquire < operation);
    assert!(begin < binding && binding < query && query < decode && decode < commit);
    let guard = include_str!("../src/connection.rs");
    assert!(guard.contains("impl Drop for ServingConnectionGuardV1"));
    assert!(guard.contains("drop(connection.detach())"));
    assert!(guard.contains("release_to_pool"));
}

#[test]
fn database_transactions_are_bounded_and_canonical() {
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
    assert!(database.contains("REPEATABLE READ READ ONLY"));
    assert!(database.contains("SERIALIZABLE READ WRITE"));
    assert!(database.contains("lock_timeout >= statement_timeout"));
}

#[test]
fn database_readiness_rejects_every_foreign_database_capability() {
    let migration =
        include_str!("../../../migrations/202607220029_scope_runtime_serving_database.sql");
    let readiness = migration
        .split("CREATE FUNCTION public.starring_runtime_serving_database_readiness_v1()")
        .nth(1)
        .unwrap();
    assert!(readiness.contains("FROM pg_catalog.pg_database AS foreign_database"));
    assert!(readiness.contains("foreign_database.oid <> database_oid"));
    assert!(readiness.contains("foreign_database.datallowconn"));
    for privilege in ["'CONNECT'", "'CREATE'", "'TEMPORARY'"] {
        assert!(readiness.contains(privilege));
    }
}

#[test]
fn postgres_security_proof_owns_its_ephemeral_cluster() {
    let proof = include_str!("postgres_security.rs");
    for required in [
        "struct EphemeralPostgresCluster",
        "STARRING_TEST_INITDB",
        "STARRING_TEST_PG_CTL",
        "impl Drop for EphemeralPostgresCluster",
        "PathBuf::from(\"/tmp\")",
        "DirBuilderExt",
        ".mode(0o700)",
        "unix_socket_permissions=0700",
    ] {
        assert!(proof.contains(required));
    }
    assert!(!proof.contains("STARRING_TEST_DATABASE_URL"));
}

#[test]
fn errors_are_redacted_at_the_adapter_boundary() {
    let sources = [
        include_str!("../src/database.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/store.rs"),
    ];
    for source in sources {
        for leak in [
            "error.to_string()",
            "format!(\"{error}",
            "format!(\"{error:?}",
            "database.message()",
            "database.detail()",
        ] {
            assert!(!source.contains(leak), "database detail leak: {leak}");
        }
    }
}
