#[test]
fn core_does_not_depend_on_postgres_adapter() {
    let manifest = include_str!("../../automation-runtime-convergence/Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!regular.contains("automation-runtime-convergence-postgres"));
    assert!(!regular.contains("sqlx"));
}

#[test]
fn adapter_does_not_depend_on_runtime_or_product_authority_edges() {
    let manifest = include_str!("../Cargo.toml");
    let regular = manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest);
    assert!(!regular.contains("automation-runtime ="));
    assert!(!regular.contains("authoring-application"));
    assert!(!regular.contains("twilight"));
    assert!(!regular.contains("axum"));
}

#[test]
fn adapter_sources_contain_no_comments() {
    let sources = [
        include_str!("../src/artifact.rs"),
        include_str!("../src/controller.rs"),
        include_str!("../src/digest.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/evidence.rs"),
        include_str!("../src/hydration/bindings.rs"),
        include_str!("../src/hydration/contract.rs"),
        include_str!("../src/hydration/mod.rs"),
        include_str!("../src/hydration/row.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/model.rs"),
        include_str!("../src/prepare.rs"),
        include_str!("../src/projection.rs"),
        include_str!("../src/row.rs"),
        include_str!("../src/store/attempt.rs"),
        include_str!("../src/store/deployment.rs"),
        include_str!("../src/store/mod.rs"),
        include_str!("../src/store/operator.rs"),
        include_str!("../src/store/previous_serving/contract.rs"),
        include_str!("../src/store/previous_serving/mod.rs"),
        include_str!("../src/store/previous_serving/row.rs"),
        include_str!("../src/store/serving.rs"),
        include_str!("../src/store/status.rs"),
    ];
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
fn previous_serving_observation_is_a_private_fenced_capability() {
    let migration =
        include_str!("../../../migrations/202607220023_observe_previous_runtime_serving.sql");
    assert!(
        migration.contains("CREATE FUNCTION public.starring_runtime_observe_previous_serving_v1(")
    );
    assert!(migration.contains("SECURITY DEFINER\nSET search_path = pg_catalog"));
    assert!(migration.contains(
        "REVOKE ALL PRIVILEGES ON FUNCTION public.starring_runtime_observe_previous_serving_v1("
    ));
    assert!(migration.contains("FROM public.runtime_deployments"));
    assert!(migration.contains("FOR UPDATE;"));
    assert!(migration.contains("pg_catalog.pg_advisory_xact_lock"));
    assert!(migration.contains("starring-runtime-serving-slot-v1:"));
    assert!(migration.contains("database_now := pg_catalog.clock_timestamp();"));
    assert!(migration.contains("deployment_row.phase <> 'drain_requested'"));
    assert!(migration.contains(
        "deployment_row.controller_fencing_token\n            IS DISTINCT FROM expected_controller_fencing_token"
    ));
    assert!(migration
        .contains("deployment_row.previous_runtime IS DISTINCT FROM expected_previous_runtime"));
    assert!(migration.contains("serving_row.acquired_at > deployment_row.requested_at"));
    assert!(migration.contains("serving_row.last_heartbeat_at < deployment_row.requested_at"));
    assert!(migration.contains("serving_row.expires_at <= deployment_row.requested_at"));
    assert!(!migration.contains("CREATE ROLE"));
    assert!(!migration.contains("GRANT SELECT"));
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }

    let contract = include_str!("../src/store/previous_serving/contract.rs");
    assert!(contract.contains("starring_runtime_observe_previous_serving_v1"));
    for relation in ["runtime_deployments", "runtime_serving_leases"] {
        assert!(!contract.contains(relation));
    }
}

#[test]
fn exact_target_hydration_is_scoped_to_private_capabilities() {
    let migration =
        include_str!("../../../migrations/202607220001_scope_runtime_exact_target_hydration.sql");
    assert_eq!(
        migration.matches("SECURITY DEFINER").count(),
        migration.matches("SET search_path = pg_catalog").count()
    );
    for function in [
        "starring_runtime_exact_target_reader_database_identity_v1",
        "starring_runtime_exact_target_read_v1",
    ] {
        assert!(migration.contains(&format!("CREATE FUNCTION public.{function}(")));
    }
    assert!(migration.contains("REVOKE ALL PRIVILEGES ON FUNCTION %s FROM PUBLIC CASCADE"));
    assert!(migration
        .contains("deployment.controller_fencing_token = expected_controller_fencing_token"));
    assert!(
        migration.contains("deployment.controller_lease_expires_at > request_clock.database_now")
    );
    assert!(migration.contains("current_authority.resource_bindings"));
    assert!(migration.contains("IS NOT DISTINCT FROM historical_authority.resource_bindings"));
    assert!(migration.contains("version.canonical_content_hash = version.content_hash"));
    for line in migration.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("/*"));
    }

    let contract = include_str!("../src/hydration/contract.rs");
    assert!(contract.contains("starring_runtime_exact_target_read_v1"));
    for relation in [
        "runtime_deployments",
        "activation_requests",
        "authoring_promotions",
        "automation_installation_authority_versions",
        "automation_ruleset_activations",
        "automation_ruleset_versions",
    ] {
        assert!(!contract.contains(relation));
    }
}

#[test]
fn runtime_security_definers_have_fixed_resolution_and_revoked_public_execution() {
    let migration = include_str!("../../../migrations/202607190002_create_runtime_convergence.sql");
    let definer_count = migration.matches("SECURITY DEFINER").count();
    assert_eq!(
        definer_count,
        migration.matches("SET search_path = pg_catalog").count()
    );
    assert!(!migration.contains("SET search_path = pg_catalog,"));
    for function in [
        "starring_runtime_lock_current_authority",
        "starring_runtime_mutation_clock",
        "starring_runtime_current_mutation_clock",
        "validate_runtime_deployment_projection",
        "reject_runtime_deployment_delete",
        "validate_runtime_attestation_projection",
        "validate_runtime_serving_lease_transition",
        "reject_runtime_serving_lease_delete",
    ] {
        assert!(migration.contains(&format!("CREATE FUNCTION public.{function}(")));
        assert!(migration.contains(&format!("REVOKE ALL ON FUNCTION public.{function}(")));
    }
}

#[test]
fn runtime_authority_rotation_preserves_historical_identity_and_current_binding() {
    let migration =
        include_str!("../../../migrations/202607190011_separate_runtime_binding_authority.sql");
    assert!(migration
        .contains("CREATE OR REPLACE FUNCTION public.starring_runtime_lock_current_authority("));
    assert!(migration.contains("SECURITY DEFINER\nSET search_path = pg_catalog"));
    assert!(migration
        .contains("REVOKE ALL ON FUNCTION public.starring_runtime_lock_current_authority("));
    assert!(!migration.contains("SET search_path = pg_catalog,"));
    assert!(!migration.contains(
        "installation_row.current_authority_revision\n        IS DISTINCT FROM expected_installation_authority_revision"
    ));
    assert!(migration.contains("revision = expected_installation_authority_revision"));
    assert!(migration.contains("revision = installation_row.current_authority_revision"));
    assert!(migration.contains(
        "historical_authority_row.binding_revision IS DISTINCT FROM expected_binding_revision"
    ));
    assert!(migration.contains(
        "historical_authority_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint"
    ));
    assert!(migration.contains(
        "current_authority_row.binding_revision IS DISTINCT FROM expected_binding_revision"
    ));
    assert!(migration.contains(
        "current_authority_row.binding_fingerprint IS DISTINCT FROM expected_binding_fingerprint"
    ));
    assert!(migration.contains(
        "current_authority_row.resource_bindings\n            IS DISTINCT FROM historical_authority_row.resource_bindings"
    ));
    assert!(!migration.contains("current_authority_row.policy_revision"));
    assert!(!migration.contains("current_authority_row.required_approvals"));
    assert!(!migration.contains("current_authority_row.activation_ttl_seconds"));
}

#[test]
fn convergence_attempt_migration_is_bounded_private_and_fail_closed() {
    let migration =
        include_str!("../../../migrations/202607200003_persist_runtime_convergence_attempts.sql");
    assert!(migration.contains("SET LOCAL lock_timeout = '5s'"));
    assert!(migration.contains("SET LOCAL statement_timeout = '30s'"));
    assert!(migration.contains("IN ACCESS EXCLUSIVE MODE"));
    assert!(migration.contains("legacy runtime execution history cannot be inferred safely"));
    assert!(migration.contains("convergence_attempt_no BIGINT NOT NULL DEFAULT 0"));
    assert!(migration.contains("last_failure_attempt_no BIGINT"));
    assert!(migration.contains("convergence_attempt_no BETWEEN 0 AND 4294967295"));
    assert!(migration.contains("convergence_attempt_no BETWEEN 1 AND 4294967295"));
    assert!(migration.contains("runtime_attestations_deployment_attempt_unique"));
    for function in [
        "validate_runtime_convergence_attempt_projection",
        "validate_runtime_attestation_attempt_projection",
    ] {
        assert!(migration.contains(&format!("CREATE FUNCTION public.{function}()")));
        assert!(migration.contains(&format!(
            "REVOKE ALL ON FUNCTION public.{function}() FROM PUBLIC"
        )));
    }
    assert_eq!(
        migration.matches("SECURITY DEFINER").count(),
        migration.matches("SET search_path = pg_catalog").count()
    );
    assert!(!migration.contains("CREATE ROLE"));
    assert!(!migration.contains("GRANT "));
    assert!(!migration.contains("last_fencing_token AS convergence_attempt"));
    for line in migration.lines() {
        let line = line.trim_start();
        assert!(!line.starts_with("--"));
        assert!(!line.starts_with("/*"));
    }
}

#[test]
fn mutable_runtime_authority_is_locked_once_in_canonical_order() {
    let migration = include_str!("../../../migrations/202607190002_create_runtime_convergence.sql");
    let authority = migration
        .split("CREATE FUNCTION public.starring_runtime_lock_current_authority(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    let mut cursor = 0;
    for table in [
        "public.activation_requests",
        "public.authoring_promotions",
        "public.product_tenants",
        "public.automation_installations",
        "public.automation_ruleset_activations",
    ] {
        let offset = authority[cursor..].find(table).unwrap();
        cursor += offset + table.len();
    }
    assert_eq!(authority.matches("FOR SHARE;").count(), 5);
    assert!(!authority.contains("FOR KEY SHARE;"));
    assert!(authority.contains("tenant_row.lifecycle_state <> 'active'"));
    assert!(authority.contains("installation_row.lifecycle_state <> 'active'"));
    let deployment_trigger = migration
        .split("CREATE FUNCTION public.validate_runtime_deployment_projection()")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap();
    assert!(deployment_trigger.contains("public.starring_runtime_lock_current_authority("));
    assert!(!deployment_trigger.contains("FROM public.activation_requests"));
    assert!(migration.contains("FROM public.runtime_serving_leases\n            WHERE guild_id = OLD.guild_id\n                AND ruleset_key = OLD.ruleset_key\n            FOR SHARE;"));
    assert!(migration.contains("FROM public.runtime_deployments\n    WHERE tenant_id = NEW.tenant_id\n        AND installation_id = NEW.installation_id\n        AND deployment_id = NEW.deployment_id\n    FOR SHARE;"));
}

#[test]
fn adapter_sql_does_not_depend_on_session_search_path() {
    let sources = [
        include_str!("../src/store/mod.rs"),
        include_str!("../src/store/deployment.rs"),
        include_str!("../src/store/serving.rs"),
        include_str!("../src/store/status.rs"),
    ];
    for source in sources {
        assert!(!source.contains("SELECT clock_timestamp()"));
        assert!(!source.contains(" set_config("));
        assert!(!source.contains(" #>> "));
        for table in [
            "runtime_deployments",
            "runtime_attestations",
            "runtime_serving_leases",
            "activation_requests",
            "authoring_promotions",
            "product_tenants",
            "automation_installations",
            "automation_installation_authority_versions",
            "automation_ruleset_activations",
            "automation_ruleset_versions",
        ] {
            for keyword in ["FROM", "JOIN", "UPDATE", "INTO"] {
                assert!(!source.contains(&format!("{keyword} {table}")));
            }
        }
    }
    let status = include_str!("../src/store/status.rs");
    let store = include_str!("../src/store/mod.rs");
    let serving = include_str!("../src/store/serving.rs");
    let deployment = include_str!("../src/store/deployment.rs");
    assert!(store.contains("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"));
    assert!(!store.contains("REPEATABLE READ READ ONLY"));
    assert!(store.contains("deployment_id = $3 FOR SHARE"));
    assert!(status.contains("ruleset_key = $2 FOR SHARE"));
    assert!(!serving.contains("LIMIT 1 FOR KEY SHARE"));
    for source in [deployment, serving] {
        assert!(!source.contains(
            "installation.current_authority_revision = deployment.installation_authority_revision"
        ));
        assert!(source.contains(
            "historical_authority.revision = deployment.installation_authority_revision"
        ));
        assert!(
            source.contains("current_authority.revision = installation.current_authority_revision")
        );
        assert!(source.contains("current_authority.binding_revision = deployment.binding_revision"));
        assert!(source
            .contains("current_authority.binding_fingerprint = deployment.binding_fingerprint"));
        assert!(source.contains("current_authority.resource_bindings"));
        assert!(source.contains("IS NOT DISTINCT FROM historical_authority.resource_bindings"));
    }
}
