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
    assert!(store.contains("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"));
    assert!(!store.contains("REPEATABLE READ READ ONLY"));
    assert!(store.contains("deployment_id = $3 FOR SHARE"));
    assert!(status.contains("ruleset_key = $2 FOR SHARE"));
    assert!(!serving.contains("LIMIT 1 FOR KEY SHARE"));
}
