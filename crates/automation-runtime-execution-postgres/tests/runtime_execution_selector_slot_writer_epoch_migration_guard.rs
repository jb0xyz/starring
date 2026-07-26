const MIGRATION: &str = include_str!(
    "../../../migrations/202607240014_fence_runtime_execution_selector_slot_writer_epoch.sql"
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

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

#[test]
fn migration_is_atomic_quiesced_rerun_closed_and_comment_free() {
    let barrier = MIGRATION.find("pg_advisory_xact_lock(").unwrap();
    let table_lock = MIGRATION.find("LOCK TABLE").unwrap();
    let snapshot = MIGRATION
        .find("starring_runtime_execution_selector_epoch_snapshot")
        .unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let claim = MIGRATION
        .find("CREATE OR REPLACE FUNCTION public.starring_runtime_execution_claim_next_v1")
        .unwrap();
    let recover = MIGRATION
        .find(
            "CREATE OR REPLACE FUNCTION public.\
starring_runtime_execution_recover_stale_live_v1",
        )
        .unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(barrier < table_lock);
    assert!(table_lock < snapshot);
    assert!(snapshot < preflight);
    assert!(preflight < claim);
    assert!(claim < recover);
    assert!(recover < postflight);
    for required in [
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '30s';",
        "IN ACCESS EXCLUSIVE MODE;",
        "function_oid OID PRIMARY KEY",
        "function_acl ACLITEM[]",
        "function_row.proacl IS DISTINCT FROM snapshot.function_acl",
        "runtime_execution_selector_slot_writer_epoch_executor_not_quiesced",
        "NOT role.rolcanlogin",
        "FROM pg_catalog.pg_auth_members AS membership",
        "FROM pg_catalog.pg_stat_activity AS activity",
        "activity.backend_type = 'client backend'",
        "activity.pid <> pg_catalog.pg_backend_pid()",
        "FROM pg_catalog.pg_prepared_xacts AS prepared",
        "prepared.database = pg_catalog.current_database()",
        "RESET statement_timeout;",
        "RESET lock_timeout;",
        "RESET search_path;",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(!MIGRATION.contains("GRANT "));
    assert!(!MIGRATION.contains("ALTER DEFAULT PRIVILEGES"));
}

#[test]
fn claim_replay_and_fresh_paths_use_canonical_slot_fencing() {
    let body =
        function_body("CREATE OR REPLACE FUNCTION public.starring_runtime_execution_claim_next_v1");
    let global = body
        .find("starring_runtime_writer_fence_observe_v1")
        .unwrap();
    let controller = body
        .find("starring-runtime-execution-controller-v1:")
        .unwrap();
    let slot = body.find("starring-runtime-serving-slot-v1:").unwrap();
    let physical = body
        .find("starring_runtime_slot_writer_fence_lock_v2")
        .unwrap();
    let deployment_lock = body.find("FOR UPDATE;").unwrap();
    let replay = body.find("outcome_name := 'replayed'").unwrap();
    let pending = body
        .find("IF pending_drain_intent_id IS NOT NULL THEN")
        .unwrap();
    let begin = body
        .find("starring_runtime_slot_writer_fence_begin_unsafe_v2")
        .unwrap();
    let mutation = body
        .find("UPDATE public.runtime_deployments AS deployment")
        .unwrap();
    assert!(global < controller);
    assert!(controller < slot);
    assert!(slot < physical);
    assert!(physical < deployment_lock);
    assert!(deployment_lock < replay);
    assert!(replay < pending);
    assert!(pending < begin);
    assert!(begin < mutation);
    assert_eq!(
        body.matches("starring_runtime_slot_writer_fence_lock_v2")
            .count(),
        2
    );
    assert_eq!(
        body.matches("starring_runtime_slot_writer_fence_begin_unsafe_v2")
            .count(),
        1
    );
    assert_eq!(body.matches("WHEN no_data_found THEN").count(), 2);
    assert_eq!(body.matches("LIMIT 64").count(), 1);
    let pending_join = body
        .find("LEFT JOIN public.runtime_drain_intents_v2 AS pending_drain")
        .unwrap();
    let pending_fence = body
        .find("AND slot_fence.pending_drain_intent_id IS NULL")
        .unwrap();
    let pending_intent = body
        .find("AND pending_drain.drain_intent_id IS NULL")
        .unwrap();
    let candidate_bound = body.find("LIMIT 64").unwrap();
    assert!(pending_join < pending_fence);
    assert!(pending_fence < pending_intent);
    assert!(pending_intent < candidate_bound);
    assert!(body.contains("WHERE deployment.tenant_id = candidate_row.tenant_id"));
    assert!(body.contains("AND deployment.deployment_id = candidate_row.deployment_id"));
    assert!(body.contains("AND deployment.revision = candidate_row.revision"));
    assert!(body.contains("AND deployment IS NOT DISTINCT FROM candidate_row"));
    assert!(body.contains("pg_catalog.pg_try_advisory_xact_lock("));
}

#[test]
fn recovery_skips_pending_or_contended_candidates_without_leaking_locks() {
    let body = function_body(
        "CREATE OR REPLACE FUNCTION public.\
starring_runtime_execution_recover_stale_live_v1",
    );
    let global = body
        .find("starring_runtime_writer_fence_observe_v1")
        .unwrap();
    let slot = body.find("starring-runtime-serving-slot-v1:").unwrap();
    let physical = body
        .find("starring_runtime_slot_writer_fence_lock_v2")
        .unwrap();
    let deployment_lock = body.find("FOR UPDATE SKIP LOCKED;").unwrap();
    let begin = body
        .find("starring_runtime_slot_writer_fence_begin_unsafe_v2")
        .unwrap();
    let mutation = body
        .find("UPDATE public.runtime_deployments AS deployment")
        .unwrap();
    assert!(global < slot);
    assert!(slot < physical);
    assert!(physical < deployment_lock);
    assert!(deployment_lock < begin);
    assert!(begin < mutation);
    assert_eq!(
        body.matches("starring_runtime_slot_writer_fence_lock_v2")
            .count(),
        1
    );
    assert_eq!(
        body.matches("starring_runtime_slot_writer_fence_begin_unsafe_v2")
            .count(),
        1
    );
    assert_eq!(body.matches("WHEN no_data_found THEN").count(), 1);
    assert_eq!(body.matches("LIMIT 64").count(), 1);
    let pending_join = body
        .find("LEFT JOIN public.runtime_drain_intents_v2 AS pending_drain")
        .unwrap();
    let pending_fence = body
        .find("AND slot_fence.pending_drain_intent_id IS NULL")
        .unwrap();
    let pending_intent = body
        .find("AND pending_drain.drain_intent_id IS NULL")
        .unwrap();
    let candidate_bound = body.find("LIMIT 64").unwrap();
    assert!(pending_join < pending_fence);
    assert!(pending_fence < pending_intent);
    assert!(pending_intent < candidate_bound);
    assert!(body.contains("IF pending_drain_intent_id IS NOT NULL THEN"));
    assert!(body.contains("RAISE no_data_found;"));
    assert!(body.contains("pg_catalog.pg_try_advisory_xact_lock("));
    assert!(!body.contains("PERFORM pg_catalog.pg_advisory_xact_lock("));
    assert!(body.contains("AND deployment IS NOT DISTINCT FROM candidate_row"));
}

#[test]
fn migration_pins_predecessor_result_and_manifest_readiness_cascade() {
    let preflight = dollar_block("preflight");
    let postflight = dollar_block("postflight");
    for digest in [
        "d1f07b5cbfb75468f37679567c2512b7f6d0555f31ee5f4f5353ad274e07aadd",
        "506c532c275fbbe51b1e67d463bf0ddfc71a258e79a6594ab19ea2235c07fc6a",
        "0d0adb92217032ac62b996a0b3e6cb3cdb3ff99a0be983626aa5df4777c78bb7",
        "b5362bc1b081789a5b3ac4881fc2ea00c340a013630f7d5c809958ed1c045ec3",
    ] {
        assert!(preflight.contains(digest), "{digest}");
    }
    for digest in [
        "7cb6550864ed68e136e6e6b48c8cce59d179d895e3919a6abca77b7dfc7a4990",
        "635aab9493e1fd2ad8a138633d6447f88752589414a27c2bad4e56afdd22f932",
        "3c97b3b41f45b11ed2b01890c3d708806d802593f71589031cb921dfc5c65fe3",
        "c5a1eb3ae9a229c127a804f6f05298ff9f797604646de202ba1a832012e7bd91",
    ] {
        assert!(postflight.contains(digest), "{digest}");
    }
    assert!(MIGRATION.contains("e9af146803f79bf195250ac230a9c39d7eef4f29349ac08a9d1c3914187fd3f2"));
    let readiness = "c5a1eb3ae9a229c127a804f6f05298ff9f797604646de202ba1a832012e7bd91";
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(readiness));
    }
}
