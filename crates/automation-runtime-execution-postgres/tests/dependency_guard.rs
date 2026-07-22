use std::fs;
use std::path::Path;

fn regular_dependencies(manifest: &str) -> &str {
    manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest)
}

fn rust_sources(directory: &Path) -> Vec<String> {
    let mut pending = vec![directory.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(fs::read_to_string(path).unwrap());
            }
        }
    }
    sources.sort_unstable();
    sources
}

#[test]
fn adapter_dependency_surface_is_narrow() {
    let manifest = include_str!("../Cargo.toml");
    let regular = regular_dependencies(manifest);
    for required in [
        "automation-runtime-controller",
        "automation-runtime-worker",
        "sqlx",
        "tokio",
    ] {
        assert!(regular.contains(required), "missing dependency: {required}");
    }
    for forbidden in [
        "automation-runtime =",
        "automation-runtime-convergence-postgres",
        "automation-runtime-serving-postgres",
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
    assert!(!regular_dependencies(manifest).contains("automation-runtime-execution-postgres"));
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
fn adapter_contract_is_function_only_and_manifest_is_isolated() {
    let contract = include_str!("../src/contract.rs");
    for capability in [
        "starring_runtime_execution_database_readiness_v1",
        "starring_runtime_execution_database_identity_v1",
        "starring_runtime_execution_claim_next_v1",
        "starring_runtime_execution_renew_v1",
        "starring_runtime_execution_mutate_v1",
        "starring_runtime_execution_certify_prepare_v1",
        "starring_runtime_execution_certify_commit_v1",
        "starring_runtime_execution_recover_stale_live_v1",
        "starring_runtime_observe_previous_serving_v1",
        "starring_runtime_gateway_owner_observe_v1",
        "starring_runtime_gateway_owner_acquire_v1",
        "starring_runtime_gateway_owner_renew_v1",
        "starring_runtime_gateway_owner_release_v1",
    ] {
        assert!(contract.contains(capability));
    }
    for forbidden in [
        "runtime_deployments",
        "runtime_attestations",
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
    assert!(contract.contains("FOUNDATIONAL_CAPABILITY_IDENTITIES_V1"));
    assert!(contract.contains("OPERATION_CAPABILITY_IDENTITIES_V1"));
}

#[test]
fn mutation_operation_uses_only_the_scoped_function_and_verified_transaction_path() {
    let query = include_str!("../src/query.rs");
    let store = include_str!("../src/store.rs");
    assert!(query.contains("starring_runtime_execution_mutate_v1"));
    assert!(query.contains("$10"));
    assert!(store.contains("pub async fn mutate("));
    assert!(store.contains("begin_execution_mutation_transaction"));
    assert!(store.contains("verify_runtime_execution_binding_v1"));
    assert!(store.contains("map_mutation_commit_error"));
    assert!(store.contains("RuntimeMutationOperationRowV1"));
    for forbidden in [
        "INSERT INTO public.runtime_deployments",
        "UPDATE public.runtime_deployments",
        "DELETE FROM public.runtime_deployments",
    ] {
        assert!(!store.contains(forbidden));
    }
}

#[test]
fn execution_metadata_and_controller_lookup_preserve_capability_ownership() {
    let migration =
        include_str!("../../../migrations/202607220030_scope_runtime_execution_database.sql");
    let exact_target =
        include_str!("../../../migrations/202607220028_scope_runtime_exact_target_database.sql");
    let serving =
        include_str!("../../../migrations/202607220029_scope_runtime_serving_database.sql");
    let ci = include_str!("../../../.github/workflows/ci.yml");
    assert!(migration.contains("CREATE TABLE public.runtime_execution_mutation_markers"));
    assert!(migration.contains("FROM public.runtime_execution_mutation_markers AS marker"));
    assert!(migration.contains("INSERT INTO public.runtime_execution_mutation_markers AS marker"));
    assert!(migration.contains(
        "CREATE FUNCTION public.validate_runtime_execution_mutation_marker_transition()"
    ));
    assert!(
        migration.contains("CREATE TRIGGER runtime_execution_mutation_markers_validate_transition")
    );
    assert!(migration
        .contains("CREATE FUNCTION public.reject_runtime_execution_mutation_marker_delete()"));
    assert!(migration.contains("CREATE TRIGGER runtime_execution_mutation_markers_reject_delete"));
    assert!(migration.contains("NEW.deployment_id IS DISTINCT FROM OLD.deployment_id"));
    assert!(migration.contains("NEW.mutation_revision <= OLD.mutation_revision"));
    assert!(migration.contains("NEW.mutation_revision IS DISTINCT FROM deployment_revision"));
    assert!(
        migration.contains("WHERE deployment.deployment_id = NEW.deployment_id\n    FOR UPDATE")
    );
    assert!(migration.contains("CREATE INDEX runtime_deployments_active_controller_index"));
    assert!(migration.contains("replay_lookup_clock := pg_catalog.clock_timestamp()"));
    assert_eq!(
        migration
            .matches("replay_validation_clock := GREATEST(")
            .count(),
        2
    );
    assert_eq!(
        migration
            .matches("controller_lease_expires_at\n            > replay_lookup_clock")
            .count(),
        1
    );
    assert_eq!(
        migration
            .matches("controller_lease_expires_at\n                    > replay_validation_clock")
            .count(),
        1
    );
    for manifest in [exact_target, serving] {
        assert!(manifest.contains("permitted_external_index(index_oid)"));
        assert!(manifest.contains("runtime_deployments_active_controller_index"));
        assert!(manifest.contains("index_contract.indexrelid NOT IN"));
        assert!(manifest.contains("index_contract.indnkeyatts = 4"));
        assert!(manifest.contains("= '(controller_id IS NOT NULL)'"));
    }
    assert!(!migration.contains("last_execution_mutation_revision"));
    assert!(!migration.contains("last_execution_mutation_kind"));
    assert!(!migration.contains("last_execution_mutation_payload"));
    assert!(ci.contains(
        "cargo test --locked -p automation-runtime-execution-postgres --test postgres_security -- --ignored --test-threads=1"
    ));
}

#[test]
fn adapter_exposes_only_execution_observation_and_gateway_owner_ports() {
    let sources = rust_sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    let source = sources.join("\n");
    for required in [
        "impl RuntimeExecutionConvergencePort",
        "impl RuntimePreviousServingObservationPort",
        "impl RuntimeGatewayOwnerLeasePortV1",
    ] {
        assert!(source.contains(required), "missing port: {required}");
    }
    assert!(!source.contains("impl RuntimeServingLeasePort"));
    assert!(source.contains("execute_certification_v1"));
    assert!(source.contains("execute_observe_previous_serving_v1"));
    assert!(source.contains("execute_recover_next_stale_live_v1"));
    assert!(source.contains("!matches!(self, Self::Observe { .. })"));
}

#[test]
fn gateway_owner_operations_use_only_scoped_functions_and_verified_transactions() {
    let gateway_owner = include_str!("../src/gateway_owner/mod.rs");
    let query = include_str!("../src/gateway_owner/query.rs");
    for capability in [
        "starring_runtime_gateway_owner_observe_v1",
        "starring_runtime_gateway_owner_acquire_v1",
        "starring_runtime_gateway_owner_renew_v1",
        "starring_runtime_gateway_owner_release_v1",
    ] {
        assert!(query.contains(capability));
    }
    for required in [
        "begin_execution_mutation_transaction",
        "verify_runtime_execution_binding_v1",
        "accept_gateway_owner_observation_v1",
        "accept_gateway_owner_acquire_v1",
        "accept_gateway_owner_renew_v1",
        "accept_gateway_owner_release_v1",
        "DefinitelyNotApplied",
        "OutcomeUnknown",
    ] {
        assert!(gateway_owner.contains(required));
    }
    for forbidden in [
        "runtime_gateway_owner_slots",
        "INSERT ",
        "UPDATE ",
        "DELETE ",
    ] {
        assert!(!gateway_owner.contains(forbidden));
    }
}

#[test]
fn adapter_cannot_be_constructed_without_readiness_verification() {
    let store = include_str!("../src/store.rs");
    let constructor = store.find("pub async fn connect_verified(").unwrap();
    let verification = store[constructor..]
        .find("verify_runtime_execution_database_with_timeouts_v1")
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
fn readiness_uses_one_absolute_deadline_and_cancellation_fencing() {
    let database = include_str!("../src/database.rs");
    let deadline = database
        .find("let deadline = tokio::time::Instant::now()")
        .unwrap();
    let acquire = database.find("pool.acquire()").unwrap();
    let operation = database
        .find("verify_runtime_execution_database_on_connection_v1")
        .unwrap();
    assert!(deadline < acquire && acquire < operation);
    assert!(database.contains("REPEATABLE READ READ ONLY"));
    assert!(database.contains("DATABASE_READINESS_DEFINITION_QUERY"));
    assert!(database.contains("RUNTIME_EXECUTION_READINESS_DEFINITION_DIGEST_V1"));
    assert!(database.contains("DATABASE_READINESS_QUERY"));
    let guard = include_str!("../src/connection.rs");
    assert!(guard.contains("impl Drop for ExecutionConnectionGuardV1"));
    assert!(guard.contains("drop(connection.detach())"));
    assert!(guard.contains("release_to_pool"));
}

#[test]
fn database_identity_observation_is_bounded_read_only_and_adapter_owned() {
    let bootstrap = include_str!("../src/bootstrap.rs");
    let production = bootstrap.split("#[cfg(test)]").next().unwrap();
    let deadline = bootstrap
        .find("let deadline = tokio::time::Instant::now()")
        .unwrap();
    let acquire = bootstrap.find("pool.acquire()").unwrap();
    let operation = bootstrap.find("identify_on_connection").unwrap();
    assert!(deadline < acquire && acquire < operation);
    assert!(bootstrap.contains("REPEATABLE READ READ ONLY"));
    assert!(bootstrap.contains("DATABASE_BINDING_QUERY"));
    assert!(bootstrap.contains("configure_read_transaction"));
    assert!(bootstrap.contains("RuntimeExecutionDatabaseBindingRowV1"));
    assert!(bootstrap.contains("verify_database_authority"));
    for forbidden in [
        "INSERT ",
        "UPDATE ",
        "DELETE ",
        "RuntimeExecutionConvergencePort",
        "RuntimeServingLeasePort",
    ] {
        assert!(
            !production.contains(forbidden),
            "bootstrap edge: {forbidden}"
        );
    }
}

#[test]
fn readiness_transaction_is_bounded_and_canonical() {
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
    assert!(database.contains("lock_timeout >= statement_timeout"));
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
