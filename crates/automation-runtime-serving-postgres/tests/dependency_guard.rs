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
        "automation-runtime-controller",
        "automation-runtime-convergence",
        "serde_json",
        "sqlx",
        "tokio",
    ] {
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
        "starring_runtime_serving_observe_v2",
        "starring_runtime_serving_heartbeat_v2",
        "starring_runtime_serving_disconnect_if_current_v2",
        "starring_runtime_serving_observe_pending_drain_source_v1",
        "starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1",
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
        ("OBSERVE_V2_QUERY", 8),
        ("HEARTBEAT_V2_QUERY", 10),
        ("DISCONNECT_V2_QUERY", 9),
        ("OBSERVE_PENDING_DRAIN_SOURCE_QUERY", 3),
        ("DISCONNECT_PENDING_DRAIN_SOURCE_IF_EXPIRED_QUERY", 18),
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
fn v2_serving_uses_exact_certified_identity_and_bounded_mutations() {
    let adapter = include_str!("../src/v2.rs");
    for required in [
        "identity.operation_id.as_str()",
        "identity.attestation_digest.as_str()",
        "identity.process_identity.process_instance_id",
        "identity.process_identity.runtime_generation",
        "identity.lease_epoch",
        "identity.revision",
        "target.version",
        "target.content_hash",
        "target.binding_revision",
        "target.binding_fingerprint",
        "MAX_RUNTIME_SERVING_LEASE_DURATION",
        "MIN_RUNTIME_SERVING_LEASE_DURATION",
        "ServingConnectionGuardV1",
        "verify_runtime_serving_binding_v1",
    ] {
        assert!(adapter.contains(required), "{required}");
    }
    assert!(adapter.contains("RuntimeServingObservationV2::Absent"));
    assert!(adapter.contains("RuntimeServingObservationV2::Current"));
    assert!(adapter.contains("RuntimeServingObservationV2::Diverged"));
}

#[test]
fn pending_drain_observation_is_exact_read_only_and_database_clocked() {
    let migration = include_str!(
        "../../../migrations/202608030002_add_pending_drain_source_serving_observation_v1.sql"
    );
    let capability = migration
        .split("CREATE FUNCTION public.starring_runtime_serving_observe_pending_drain_source_v1(")
        .nth(1)
        .and_then(|source| source.split("$function$;").next())
        .unwrap();
    for required in [
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "pg_catalog.current_setting('transaction_isolation')",
        "<> 'serializable'",
        "pg_catalog.current_setting('transaction_read_only') <> 'off'",
        "runtime_pending_drain_source_serving_observation_transaction_drift",
        "expected_drain_intent_id",
        "expected_source_intent_revision",
        "expected_source_state_digest",
        "starring_runtime_pending_drain_state_exact_v2",
        "drain_row.canonical_state_digest",
        "pg_catalog.sha256(drain_row.canonical_state_bytes)",
        "runtime_slot_writer_fences_v2",
        "pending_drain_intent_id",
        "pending_product_operation_id",
        "runtime_deployments",
        "deployment_row.phase <> 'live'",
        "runtime_attestations",
        "record_format_version <> 2",
        "runtime_serving_leases",
        "pg_catalog.clock_timestamp()",
        "outcome_name := 'absent'",
        "outcome_name := 'current'",
        "outcome_name := 'diverged'",
    ] {
        assert!(capability.contains(required), "{required}");
    }
    for forbidden in [
        "INSERT INTO",
        "UPDATE public.",
        "DELETE FROM",
        "TRUNCATE ",
        "FOR UPDATE",
        "FOR SHARE",
        "FOR KEY SHARE",
    ] {
        assert!(!capability.contains(forbidden), "{forbidden}");
    }
    let adapter = include_str!("../src/pending_drain.rs");
    let method = adapter
        .split("pub async fn observe_pending_drain_source_serving_v1(")
        .nth(1)
        .unwrap();
    let acquire = method.find("self.pool.acquire()").unwrap();
    let begin = method.find("begin_serving_mutation_transaction").unwrap();
    let binding = method.find("verify_runtime_serving_binding_v1").unwrap();
    let query = method.find("OBSERVE_PENDING_DRAIN_SOURCE_QUERY").unwrap();
    let decode = method.find("row.decode(lookup)").unwrap();
    let commit = method.find("transaction.commit()").unwrap();
    assert!(
        acquire < begin && begin < binding && binding < query && query < decode && decode < commit
    );
}

#[test]
fn pending_drain_observation_rejects_cross_scope_serving() {
    let migration = include_str!(
        "../../../migrations/202608030002_add_pending_drain_source_serving_observation_v1.sql"
    );
    let capability = migration
        .split("CREATE FUNCTION public.starring_runtime_serving_observe_pending_drain_source_v1(")
        .nth(1)
        .and_then(|source| source.split("$function$;").next())
        .unwrap();
    for exact_scope_guard in [
        "fence_row.pending_tenant_id\n            IS DISTINCT FROM drain_row.tenant_id",
        "fence_row.pending_installation_id\n            IS DISTINCT FROM drain_row.installation_id",
        "fence_row.pending_deployment_id\n            IS DISTINCT FROM drain_row.deployment_id",
        "serving_row.tenant_id IS DISTINCT FROM drain_row.tenant_id",
        "serving_row.installation_id\n            IS DISTINCT FROM drain_row.installation_id",
        "serving_row.deployment_id\n            IS DISTINCT FROM drain_row.deployment_id",
        "serving_row.attestation_id\n            IS DISTINCT FROM attestation_row.attestation_id",
        "serving_row.process_instance_id\n            IS DISTINCT FROM attestation_row.process_instance_id",
        "serving_row.runtime_generation\n            IS DISTINCT FROM attestation_row.runtime_generation",
    ] {
        assert!(capability.contains(exact_scope_guard), "{exact_scope_guard}");
    }
}

#[test]
fn pending_drain_expired_disconnect_is_atomic_exact_and_writer_fenced() {
    let migration = include_str!(
        "../../../migrations/202608030002_add_pending_drain_source_serving_observation_v1.sql"
    );
    let capability = migration
        .split(
            "CREATE FUNCTION public.starring_runtime_serving_disconnect_pending_drain_source_if_expired_v1(",
        )
        .nth(1)
        .and_then(|source| source.split("$function$;").next())
        .unwrap();
    for required in [
        "SECURITY DEFINER",
        "SET search_path = pg_catalog",
        "pg_catalog.current_setting('transaction_isolation')",
        "<> 'serializable'",
        "pg_catalog.current_setting('transaction_read_only') <> 'off'",
        "runtime_pending_drain_source_serving_disconnect_transaction_drift",
        "pg_catalog.pg_advisory_xact_lock_shared",
        "starring-runtime-writer-fence-v1",
        "public.runtime_writer_fence",
        "writer_fence_count <> 1",
        "writer_fence_state NOT IN ('open', 'closed')",
        "writer_fence_state = 'closed'",
        "ERRCODE = 'RS005'",
        "starring-runtime-serving-slot-v1:",
        "starring_runtime_pending_drain_state_exact_v2",
        "fence_row.pending_drain_intent_id",
        "deployment_row.phase <> 'live'",
        "attestation_row.record_format_version <> 2",
        "serving_row.revision = expected_serving_revision",
        "serving_row.expires_at > observed_at",
        "public.starring_runtime_mutation_clock()",
        "UPDATE public.runtime_serving_leases AS lease",
        "SET revision = next_revision",
        "AND lease.revision = expected_serving_revision",
        "AND lease.expires_at <= mutation_clock",
        "serving_row.revision = expected_serving_revision + 1",
        "serving_row.last_heartbeat_at\n                IS DISTINCT FROM serving_row.expires_at",
        "OR serving_row.expires_at > observed_at",
    ] {
        assert!(capability.contains(required), "{required}");
    }
    for forbidden in [
        "FOR UPDATE",
        "FOR SHARE",
        "FOR KEY SHARE",
        "starring_runtime_serving_disconnect_if_current_v2",
        "DELETE FROM",
        "INSERT INTO",
    ] {
        assert!(!capability.contains(forbidden), "{forbidden}");
    }
    let global_lock = capability
        .find("pg_catalog.pg_advisory_xact_lock_shared")
        .unwrap();
    let writer_fence = capability.find("FROM public.runtime_writer_fence").unwrap();
    let slot_lock = capability
        .find("'starring-runtime-serving-slot-v1:'")
        .unwrap();
    let first_source_read = capability
        .find("FROM public.runtime_drain_intents_v2")
        .unwrap();
    let mutation = capability
        .find("UPDATE public.runtime_serving_leases AS lease")
        .unwrap();
    assert!(
        global_lock < writer_fence
            && writer_fence < slot_lock
            && slot_lock < first_source_read
            && first_source_read < mutation
    );

    let adapter = include_str!("../src/pending_drain.rs");
    let method = adapter
        .split("pub async fn disconnect_pending_drain_source_serving_if_expired_v1(")
        .nth(1)
        .unwrap();
    let acquire = method.find("self.pool.acquire()").unwrap();
    let begin = method.find("begin_serving_mutation_transaction").unwrap();
    let binding = method.find("verify_runtime_serving_binding_v1").unwrap();
    let query = method
        .find("DISCONNECT_PENDING_DRAIN_SOURCE_IF_EXPIRED_QUERY")
        .unwrap();
    let first_bind = method.find(".bind(lookup.intent_id.as_str())").unwrap();
    let final_bind = method
        .find(".bind(runtime_i64(identity.revision.get())?)")
        .unwrap();
    let decode = method.find("row.decode_disconnect(identity)").unwrap();
    let commit = method.find("map_err(map_mutation_commit_error)").unwrap();
    assert!(
        acquire < begin
            && begin < binding
            && binding < query
            && query < first_bind
            && first_bind < final_bind
            && final_bind < decode
            && decode < commit
    );
    assert!(method.contains("RuntimeServingPersistenceErrorV1::Indeterminate"));
    for exact_bind in [
        ".bind(lookup.intent_id.as_str())",
        "runtime_i64(lookup.source_intent_revision.get())?",
        ".bind(lowercase_hex(&lookup.source_state_digest))",
        ".bind(identity.operation_id.as_str())",
        ".bind(identity.scope.tenant_id.as_str())",
        ".bind(identity.scope.installation_id.as_str())",
        ".bind(identity.scope.deployment_id.as_str())",
        ".bind(target.guild_id.to_string())",
        ".bind(target.ruleset_key.as_str())",
        ".bind(identity.attestation_digest.as_str())",
        ".bind(identity.process_identity.process_instance_id.as_str())",
    ] {
        assert!(method.contains(exact_bind), "{exact_bind}");
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
fn postgres_security_proof_uses_only_strict_test_servers() {
    let proof = include_str!("postgres_security.rs");
    for required in [
        "enum PostgresTestServer",
        "STARRING_TEST_DATABASE_URL",
        "refusing to use a database outside the strict Starring test namespace",
        "Self::External(Box::new(options))",
        "Self::Ephemeral(EphemeralPostgresCluster::start())",
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
}

#[test]
fn errors_are_redacted_at_the_adapter_boundary() {
    let sources = [
        include_str!("../src/database.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/store.rs"),
        include_str!("../src/v2.rs"),
        include_str!("../src/pending_drain.rs"),
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
