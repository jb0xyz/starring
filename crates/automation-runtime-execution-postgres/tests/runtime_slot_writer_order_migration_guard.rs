const MIGRATION: &str =
    include_str!("../../../migrations/202607240003_order_runtime_slot_writers.sql");

fn function_body(name: &str) -> &'static str {
    MIGRATION
        .split(&format!("CREATE OR REPLACE FUNCTION public.{name}"))
        .nth(1)
        .unwrap()
        .split("AS $function$")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn runtime_slot_writer_order_migration_is_atomic_and_comment_free() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    let writer_barrier = MIGRATION
        .find("pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)")
        .unwrap();
    let table_barrier = MIGRATION.find("LOCK TABLE").unwrap();
    assert!(writer_barrier < table_barrier);
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE"));
    for relation in [
        "public.runtime_writer_fence",
        "public.runtime_deployments",
        "public.runtime_serving_leases",
        "public.runtime_attestations",
        "public.runtime_execution_mutation_markers",
    ] {
        assert!(MIGRATION.contains(relation), "{relation}");
    }
    assert!(
        MIGRATION.contains("CREATE TEMPORARY TABLE pg_temp.starring_runtime_lock_order_snapshot")
    );
    assert!(MIGRATION.contains("snapshot_mismatch_count <> 0"));
    assert!(MIGRATION.contains("runtime_slot_writer_order_preflight_drift"));
    assert!(MIGRATION.contains("runtime_slot_writer_order_postflight_drift"));
    assert!(!MIGRATION.contains("_unfenced_v1"));
}

#[test]
fn writer_fence_observation_is_snapshot_safe_and_missing_is_closed() {
    let body = function_body("starring_runtime_writer_fence_observe_v1()");
    let writer = body.find("pg_advisory_xact_lock_shared").unwrap();
    let fence = body
        .find("FROM public.runtime_writer_fence AS fence")
        .unwrap();
    let row_lock = body.find("FOR SHARE").unwrap();
    let missing = body.find("IF NOT FOUND THEN").unwrap();
    assert!(writer < fence);
    assert!(fence < row_lock);
    assert!(row_lock < missing);
    assert!(!body.contains("INTO STRICT"));
}

#[test]
fn every_slot_writer_takes_writer_then_slot_then_first_row_lock() {
    for (name, writer_marker) in [
        (
            "starring_runtime_observe_previous_serving_v1(",
            "starring_runtime_writer_fence_observe_v1",
        ),
        (
            "starring_runtime_execution_mutate_v1(",
            "starring_runtime_writer_fence_observe_v1",
        ),
        (
            "starring_runtime_execution_certify_prepare_v1(",
            "starring_runtime_writer_fence_observe_v1",
        ),
        (
            "starring_runtime_execution_certify_commit_v1(",
            "starring_runtime_writer_fence_observe_v1",
        ),
        (
            "starring_runtime_execution_recover_stale_live_v1()",
            "starring_runtime_writer_fence_observe_v1",
        ),
        (
            "starring_runtime_serving_heartbeat_v1(",
            "starring-runtime-writer-fence-v1",
        ),
        (
            "starring_runtime_serving_disconnect_v1(",
            "starring-runtime-writer-fence-v1",
        ),
    ] {
        let body = function_body(name);
        let writer = body.find(writer_marker).unwrap();
        let slot = body.find("starring-runtime-serving-slot-v1:").unwrap();
        let row_lock = body.find("FOR UPDATE").unwrap();
        assert!(writer < slot, "{name}");
        assert!(slot < row_lock, "{name}");
        assert!(
            body.contains("runtime_execution_writer_fenced")
                || body.contains("runtime_serving_writer_fenced")
        );
    }
}

#[test]
fn stale_recovery_scans_past_busy_slots_before_selecting_a_candidate() {
    let body = function_body("starring_runtime_execution_recover_stale_live_v1()");
    let candidate_loop = body.find("FOR candidate_row IN").unwrap();
    let candidate_limit = body.find("LIMIT 64").unwrap();
    let try_slot = body.find("pg_try_advisory_xact_lock").unwrap();
    let rollback_scope = body[candidate_loop..try_slot].rfind("BEGIN").unwrap() + candidate_loop;
    let exact_row_lock = body.find("FOR UPDATE SKIP LOCKED").unwrap();
    let rollback_rejected_slot = body.find("RAISE no_data_found").unwrap();
    let exception_handler = body.find("WHEN no_data_found THEN").unwrap();
    let candidate_selected = body.find("candidate_found := TRUE").unwrap();
    let exit = body[candidate_selected..].find("EXIT;").unwrap() + candidate_selected;
    let loop_end = body.find("END LOOP;").unwrap();
    assert!(candidate_loop < candidate_limit);
    assert!(candidate_limit < rollback_scope);
    assert!(rollback_scope < try_slot);
    assert!(try_slot < exact_row_lock);
    assert!(exact_row_lock < rollback_rejected_slot);
    assert!(rollback_rejected_slot < candidate_selected);
    assert!(candidate_selected < exception_handler);
    assert!(candidate_selected < exit);
    assert!(exception_handler < exit);
    assert!(exit < loop_end);
    assert!(!body[loop_end..].contains("FOR UPDATE SKIP LOCKED"));
    assert!(body.contains("AND deployment.revision = candidate_row.revision"));
    assert!(body.contains("AND deployment.snapshot IS NOT DISTINCT FROM candidate_row.snapshot"));
}

#[test]
fn manifests_readiness_and_rust_pins_have_no_placeholders() {
    for value in [
        "RETURN observed_count = 574",
        "849f36c9bd2d04e19008a3917aff07ede45fdda06f5bd1824b8e9c622077bc24",
        "RETURN observed_count = 469",
        "7ef840eba14126d4dae1d05ae4920858f7b72d1b4fb4f14d8477abdc65d982ea",
        "c819437ec90f4f64ebd8a3722979e2ea817e87bdc370eef1e5c196e163551188",
        "45791dded732504e4f235f17153646affa14f6e94e6b4fdc0874f2279e1533a7",
        "('public.runtime_writer_fence')",
        "('public.reject_runtime_writer_fence_mutation()')",
    ] {
        assert!(MIGRATION.contains(value), "{value}");
    }
    for placeholder in [
        "__FUNCTION_DEFINITIONS__",
        "__POSTFLIGHT__",
        "__MANIFEST_OBSERVED_DIGEST__",
        "__READINESS_DEFINITION_DIGEST__",
    ] {
        assert!(!MIGRATION.contains(placeholder), "{placeholder}");
    }
}
