const MIGRATION: &str = include_str!(
    "../../../migrations/202607270001_persist_runtime_certification_reservation_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

fn function_body(marker: &str) -> &'static str {
    MIGRATION
        .split(marker)
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
        .split("AS $function$")
        .nth(1)
        .unwrap()
}

#[test]
fn migration_is_atomic_closed_and_comment_free() {
    let global = MIGRATION.find("pg_advisory_xact_lock(").unwrap();
    let table_lock = MIGRATION.find("LOCK TABLE").unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let table = MIGRATION
        .find("CREATE TABLE public.runtime_certification_operations_v2")
        .unwrap();
    let manifest = MIGRATION.find("DO $patch_schema_manifest$").unwrap();
    let readiness = MIGRATION.find("DO $patch_readiness$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(global < table_lock);
    assert!(table_lock < preflight);
    assert!(preflight < table);
    assert!(table < manifest);
    assert!(manifest < readiness);
    assert!(readiness < postflight);
    for required in [
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '30s';",
        "IN ACCESS EXCLUSIVE MODE;",
        "runtime_certification_reservation_preflight_drift",
        "runtime_certification_reservation_postflight_drift",
        "RESET statement_timeout;",
        "RESET lock_timeout;",
        "RESET search_path;",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for forbidden in ["--", "/*", "//", "GRANT ", "ALTER DEFAULT PRIVILEGES"] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn reservation_root_is_insert_only_and_has_independent_id_and_scope_identity() {
    let table = MIGRATION
        .split("CREATE TABLE public.runtime_certification_operations_v2")
        .nth(1)
        .unwrap()
        .split(");")
        .next()
        .unwrap();
    for required in [
        "operation_id TEXT PRIMARY KEY",
        "certification_intent_bytes BYTEA NOT NULL",
        "intent_fingerprint TEXT NOT NULL",
        "runtime_certification_operations_v2_natural_unique UNIQUE",
        "tenant_id,\n        installation_id,\n        deployment_id,\n        deployment_revision,\n        convergence_attempt_no",
        "runtime_certification_operations_v2_child_unique UNIQUE",
        "operation_id ~ '^[0-9a-f]{32}$'",
        "convergence_attempt_no BETWEEN 1 AND 4294967295",
        "pg_catalog.octet_length(certification_intent_bytes)\n            BETWEEN 1 AND 32768",
        "intent_fingerprint ~ '^[0-9a-f]{64}$'",
    ] {
        assert!(table.contains(required), "{required}");
    }
    for required in [
        "BEFORE INSERT OR UPDATE OR DELETE",
        "BEFORE TRUNCATE",
        "runtime_certification_reservation_mutation_rejected",
        "starring.runtime_certification_reservation_action_v2",
        "runtime_certification_reservation_gate_drift",
        "REVOKE ALL ON TABLE\n    public.runtime_certification_operations_v2\nFROM PUBLIC;",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
}

#[test]
fn canonical_builder_matches_the_rust_v2_wire_golden() {
    let builder = function_body(
        "CREATE FUNCTION starring_runtime_private_v2.\
starring_runtime_certification_intent_bytes_v2",
    );
    for ordered_field in [
        "{\"format_version\":2,\"action_id\":",
        ",\"operation_id\":",
        ",\"guard\":{\"scope\":{\"tenant_id\":",
        "},\"expected_revision\":",
        ",\"target\":",
        ",\"binding_pin\":{\"tenant_id\":",
        ",\"process_identity\":",
        ",\"gateway_owner_lease_id\":{\"gateway_shard_id\":",
        "},\"observed_owner_revision\":",
        ",\"runtime_build_revision\":",
        ",\"panel\":{\"certificate_id\":",
        ",\"serving_lease_milliseconds\":",
    ] {
        assert!(builder.contains(ordered_field), "{ordered_field}");
    }
    assert_eq!(builder.matches("|| target_bytes").count(), 2);
    assert_eq!(builder.matches("|| process_identity_bytes").count(), 2);
    assert!(MIGRATION.contains("'starring.runtime.certification_intent.v2'"));
    assert!(MIGRATION.contains("pg_catalog.decode('00', 'hex')"));
    assert!(MIGRATION.contains("686ccbc5e00269f5b373bd5eec398e3b845e17d938cce2b4ae3e1ef19923b99d"));
    assert!(MIGRATION.contains(
        "starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2"
    ));
}

#[test]
fn reserve_uses_canonical_lock_order_and_only_fresh_insert_advances_epoch() {
    let body =
        function_body("CREATE FUNCTION public.starring_runtime_certification_reserve_intent_v2");
    let canonical = body
        .find("starring_runtime_certification_intent_bytes_v2")
        .unwrap();
    let global = body
        .find("starring_runtime_writer_fence_observe_v1")
        .unwrap();
    let slot = body.find("starring-runtime-serving-slot-v1:").unwrap();
    let physical = body
        .find("starring_runtime_slot_writer_fence_lock_v2")
        .unwrap();
    let deployment = body.find("FOR UPDATE;").unwrap();
    let replay = body.find("IF reservation_found THEN").unwrap();
    let authority = body
        .find("starring_runtime_lock_current_authority")
        .unwrap();
    let owner = body.find("starring-runtime-gateway-owner-v1:").unwrap();
    let advance = body
        .find("starring_runtime_slot_writer_fence_begin_unsafe_v2")
        .unwrap();
    let insert = body
        .find("INSERT INTO public.runtime_certification_operations_v2")
        .unwrap();
    assert!(canonical < global);
    assert!(global < slot);
    assert!(slot < physical);
    assert!(physical < deployment);
    assert!(deployment < replay);
    assert!(replay < authority);
    assert!(authority < owner);
    assert!(owner < advance);
    assert!(advance < insert);
    assert_eq!(
        body.matches("starring_runtime_slot_writer_fence_begin_unsafe_v2")
            .count(),
        1
    );
    assert!(body.contains("proposed_certification_intent_bytes IS DISTINCT FROM expected_bytes"));
    assert!(body.contains("proposed_intent_fingerprint IS DISTINCT FROM expected_fingerprint"));
    assert!(body.contains(
        "pg_catalog.current_setting('transaction_isolation')\n            <> 'serializable'"
    ));
    assert!(body.contains("pg_catalog.current_setting('transaction_read_only') <> 'off'"));
    assert!(body.contains("WHEN unique_violation THEN"));
}

#[test]
fn observation_is_scope_only_and_returns_database_time_with_locked_state() {
    let marker = "CREATE FUNCTION public.starring_runtime_certification_reservation_observe_v2";
    let signature = MIGRATION
        .split(marker)
        .nth(1)
        .unwrap()
        .split("RETURNS TABLE")
        .next()
        .unwrap();
    assert!(!signature.contains("operation_id"));
    assert!(!signature.contains("fingerprint"));
    assert!(!signature.contains("BYTEA"));
    let body = function_body(marker);
    let global = body
        .find("starring_runtime_writer_fence_observe_v1")
        .unwrap();
    let slot = body.find("starring-runtime-serving-slot-v1:").unwrap();
    let physical = body
        .find("starring_runtime_slot_writer_fence_lock_v2")
        .unwrap();
    let deployment = body.find("FOR UPDATE;").unwrap();
    let clock = body.rfind("pg_catalog.clock_timestamp()").unwrap();
    let reservation = body
        .find("FROM public.runtime_certification_operations_v2")
        .unwrap();
    assert!(global < slot);
    assert!(slot < physical);
    assert!(physical < deployment);
    assert!(deployment < clock);
    assert!(clock < reservation);
    assert!(body.contains("outcome_name := 'absent'"));
    assert!(body.contains("outcome_name := 'reserved'"));
    assert!(body.contains("outcome_name := 'diverged'"));
    assert!(body.contains(
        "pg_catalog.current_setting('transaction_isolation')\n            <> 'read committed'"
    ));
    assert!(body.contains(
        "starring_runtime_certification_intent_fingerprint_v2(\n        reservation_row.certification_intent_bytes"
    ));
}

#[test]
fn migration_creates_dormant_owner_only_capabilities() {
    for identity in [
        "public.starring_runtime_certification_reserve_intent_v2",
        "public.starring_runtime_certification_reservation_observe_v2",
        "starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2",
        "starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2",
    ] {
        assert!(MIGRATION.contains(identity), "{identity}");
    }
    assert!(MIGRATION.contains(
        "REVOKE ALL ON FUNCTION\n    public.starring_runtime_certification_reserve_intent_v2"
    ));
    assert!(MIGRATION.contains(
        "REVOKE ALL ON FUNCTION\n    public.starring_runtime_certification_reservation_observe_v2"
    ));
    assert!(MIGRATION.contains(
        "REVOKE ALL ON FUNCTION\n    starring_runtime_private_v2.starring_runtime_certification_intent_bytes_v2"
    ));
    assert!(MIGRATION.contains(
        "REVOKE ALL ON FUNCTION\n    starring_runtime_private_v2.starring_runtime_certification_intent_fingerprint_v2"
    ));
    assert!(!MIGRATION.contains("GRANT "));
    let postflight = MIGRATION
        .split("DO $postflight$")
        .nth(1)
        .unwrap()
        .split("$postflight$;")
        .next()
        .unwrap();
    assert!(postflight.contains("privilege.grantee <> common_owner"));
    assert!(postflight.contains("invalid_acl_count <> 0"));
    assert!(!MIGRATION.contains(
        "CREATE OR REPLACE FUNCTION public.starring_runtime_execution_certify_prepare_v1"
    ));
    assert!(!MIGRATION.contains(
        "CREATE OR REPLACE FUNCTION public.starring_runtime_execution_certify_commit_v1"
    ));
}

#[test]
fn manifest_and_readiness_cascade_is_exact() {
    for expected in [
        "RETURN observed_count = 650",
        "f053e9131dcd32f1168ff6201ad57f4f40e3165ab619414a3552b74717bbe2c9",
        "4089395be3df848f9025655ef183b0336ecfefd62861bf735f53c4c26aad2ae7",
        "6962c1c2ffdd862a86aed3c84569ac50307964d59711d0bddc26aadbf68577e2",
    ] {
        assert!(MIGRATION.contains(expected), "{expected}");
    }
    let readiness = "98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2";
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(readiness));
    }
}
