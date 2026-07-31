const MIGRATION: &str =
    include_str!("../../../migrations/202607300003_finalize_runtime_certification_v2.sql");
const EXECUTION_CONTRACT: &str = include_str!("../src/contract.rs");
const EXECUTION_DATABASE: &str = include_str!("../src/database.rs");
const EXECUTION_V2_ADAPTER: &str = include_str!("../src/certification_v2.rs");
const SERVING_DATABASE: &str =
    include_str!("../../automation-runtime-serving-postgres/src/database.rs");
const ROLE_BOOTSTRAP: &str =
    include_str!("../../../ops/postgres/staging-runtime-role-bootstrap.sql");

fn function(name: &str) -> &'static str {
    MIGRATION
        .split(&format!("CREATE FUNCTION public.{name}("))
        .nth(1)
        .unwrap()
        .split("\n$function$;")
        .next()
        .unwrap()
}

fn block(name: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${name}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${name}$;"))
        .next()
        .unwrap()
}

#[test]
fn attestation_v2_shadow_is_closed_and_byte_exact() {
    for required in [
        "ADD COLUMN v2_operation_id TEXT",
        "ADD COLUMN v2_request_bytes BYTEA",
        "ADD COLUMN v2_live_attestation_bytes BYTEA",
        "ADD COLUMN v2_route_admission JSONB",
        "ADD COLUMN v2_initial_lease_epoch BIGINT",
        "ADD COLUMN v2_initial_serving_revision BIGINT",
        "runtime_attestations_v2_shape_valid",
        "record_format_version = 1",
        "record_format_version = 2",
        "v2_operation_id IS NULL",
        "v2_operation_id ~ '^[0-9a-f]{32}$'",
        "starring.runtime.certification_request.v2",
        "starring.runtime.live_attestation.v2",
        "v2_live_attestation_bytes =",
        "runtime_attestations_v2_operation_unique",
        "runtime_attestations_v2_request_digest_unique",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
}

#[test]
fn finalization_commit_is_one_fenced_terminal_transaction() {
    let prepare = function("starring_runtime_certification_prepare_v2");
    let commit = function("starring_runtime_certification_commit_v2");
    for required in [
        "transaction_isolation",
        "'serializable'",
        "requested_must_commit_before",
        "controller_lease_expires_at",
        "starring_runtime_lock_current_authority",
        "runtime_certification_operation_terminals_v2",
    ] {
        assert!(prepare.contains(required), "{required}");
    }
    for required in [
        "starring.runtime.certification_request.v2",
        "starring.runtime.live_attestation.v2",
        "FOR UPDATE",
        "pending_drain_intent_id IS NOT NULL",
        "pending_product_operation_id IS NOT NULL",
        "starring_runtime_lock_current_authority",
        "INSERT INTO public.runtime_attestations",
        "INSERT INTO public.runtime_serving_leases",
        "UPDATE public.runtime_deployments",
        "INSERT INTO public.runtime_certification_operation_terminals_v2",
        "'certification_committed'",
        "'live'",
    ] {
        assert!(commit.contains(required), "{required}");
    }
    assert!(!commit.contains("runtime_ingress_open_acknowledgements_v2"));
    assert!(!commit.contains("runtime_certification_commit_v2_ingress_mismatch"));
    let attestation = commit
        .find("INSERT INTO public.runtime_attestations")
        .unwrap();
    let serving = commit
        .find("INSERT INTO public.runtime_serving_leases")
        .unwrap();
    let deployment = commit.find("UPDATE public.runtime_deployments").unwrap();
    let terminal = commit
        .find("INSERT INTO public.runtime_certification_operation_terminals_v2")
        .unwrap();
    assert!(attestation < serving);
    assert!(serving < deployment);
    assert!(deployment < terminal);
}

#[test]
fn observation_closes_commit_ack_loss_without_current_lease_dependency() {
    let observe = function("starring_runtime_certification_observe_v2");
    for required in [
        "terminal_count = 1 AND attestation_count = 1",
        "terminal_outcome_name",
        "'certification_committed'",
        "v2_request_digest",
        "expected_request_digest",
        "v2_prepared_snapshot",
        "v2_certified_snapshot",
        "v2_initial_lease_epoch",
        "v2_initial_serving_revision",
        "outcome_name := 'committed'",
        "outcome_name := 'not_committed'",
        "outcome_name := 'diverged'",
        "pg_advisory_xact_lock",
        "FOR SHARE",
    ] {
        assert!(observe.contains(required), "{required}");
    }
    assert!(!observe.contains("FROM public.runtime_serving_leases"));
    let deployment_lock = observe.find("FROM public.runtime_deployments").unwrap();
    let advisory_lock = observe.find("pg_advisory_xact_lock").unwrap();
    let terminal_count = observe.find("INTO terminal_count").unwrap();
    assert!(deployment_lock < advisory_lock);
    assert!(advisory_lock < terminal_count);
}

#[test]
fn serving_v2_requires_exact_certification_and_fresh_ingress() {
    let observe = function("starring_runtime_serving_observe_v2");
    let heartbeat = function("starring_runtime_serving_heartbeat_v2");
    let disconnect = function("starring_runtime_serving_disconnect_if_current_v2");
    for source in [observe, heartbeat, disconnect] {
        for required in [
            "v2_operation_id = expected_operation_id",
            "attestation.attestation_id",
            "expected_attestation_digest",
            "v2_initial_lease_epoch",
            "target_version",
            "target_content_hash",
            "binding_revision",
            "binding_fingerprint",
        ] {
            assert!(source.contains(required), "{required}");
        }
    }
    for required in [
        "runtime_gateway_owners",
        "runtime_ingress_open_acknowledgements_v2",
        "attested_owner_revision",
        "connected_event_sequence",
        "resume_sequence",
        "requested_lease_milliseconds * 1000000",
        "starring_runtime_serving_heartbeat_v1",
    ] {
        assert!(heartbeat.contains(required), "{required}");
    }
    assert!(disconnect.contains("starring_runtime_serving_disconnect_v1"));
    assert!(!disconnect.contains("runtime_gateway_owners"));
    assert!(!disconnect.contains("runtime_ingress_open_acknowledgements_v2"));
}

#[test]
fn adapter_failure_paths_do_not_invent_transaction_end_authority() {
    let commit = EXECUTION_V2_ADAPTER
        .split("async fn commit_live_v2(")
        .nth(1)
        .unwrap()
        .split("async fn abort(")
        .next()
        .unwrap();
    for conversion in [
        "match positive_i64(guard.expected_revision.get())",
        "match positive_i64(guard.fencing_token.get())",
        "match positive_i64(guard.runtime_generation.get())",
    ] {
        let conversion = commit.find(conversion).unwrap();
        let rollback = commit[conversion..]
            .find("rollback_before_commit(")
            .unwrap();
        assert!(rollback > 0);
    }
    assert!(!commit.contains(
        "map_err(\n                    |source| RuntimeCommitCompletionErrorV2::DefinitelyRolledBack"
    ));

    let recovery = EXECUTION_V2_ADAPTER
        .split("impl RuntimeAbortRecoveryPortV2")
        .nth(1)
        .unwrap()
        .split("impl RuntimeCommitRecoveryPortV2")
        .next()
        .unwrap();
    assert!(recovery.contains("RuntimeExecutionPersistenceErrorV1::Indeterminate"));
    assert!(recovery.contains("RuntimeRecoveryPendingV2"));
    assert!(!recovery.contains("yield_now"));
    assert!(!recovery.contains("Ok(())"));
}

#[test]
fn manifests_readiness_and_roles_expose_only_six_capabilities() {
    let manifest = block("patch_schema_manifests");
    let readiness = block("patch_readiness");
    let acl = block("capability_acl");
    for name in [
        "starring_runtime_certification_prepare_v2",
        "starring_runtime_certification_commit_v2",
        "starring_runtime_certification_observe_v2",
        "starring_runtime_serving_observe_v2",
        "starring_runtime_serving_heartbeat_v2",
        "starring_runtime_serving_disconnect_if_current_v2",
    ] {
        assert!(manifest.contains(name), "{name}");
        assert!(readiness.contains(name), "{name}");
        assert!(acl.contains(name), "{name}");
        assert!(ROLE_BOOTSTRAP.contains(name), "{name}");
    }
    assert!(acl.contains("REVOKE ALL ON FUNCTION %s FROM PUBLIC"));
    assert!(!acl.contains("GRANT SELECT"));
    assert!(!acl.contains("GRANT INSERT"));
    assert!(!acl.contains("GRANT UPDATE"));
    assert!(!acl.contains("GRANT DELETE"));
    assert!(ROLE_BOOTSTRAP.contains(") <> 60 THEN"));
    assert!(EXECUTION_CONTRACT
        .contains("0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f"));
    assert!(EXECUTION_DATABASE
        .contains("0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f"));
    assert!(SERVING_DATABASE
        .contains("16ac5e4726c5ab72da45c1ab67490a50e737197d79a435133fcbd27b56f79a15"));
}
