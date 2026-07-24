const MIGRATION: &str =
    include_str!("../../../migrations/202607240013_fence_runtime_execution_slot_writer_epoch.sql");
const ERROR_SOURCE: &str = include_str!("../src/error.rs");
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

fn block<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).unwrap();
    let end = source[start..].find(end).unwrap() + start + end.len();
    &source[start..end]
}

#[test]
fn migration_is_atomic_preflighted_bounded_and_comment_free() {
    let barrier = MIGRATION.find("pg_advisory_xact_lock(").unwrap();
    let table_lock = MIGRATION.find("LOCK TABLE").unwrap();
    let snapshot = MIGRATION
        .find("starring_runtime_execution_slot_epoch_snapshot")
        .unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let patch = MIGRATION.find("DO $patch_renew$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(barrier < table_lock);
    assert!(table_lock < snapshot);
    assert!(snapshot < preflight);
    assert!(preflight < patch);
    assert!(patch < postflight);
    for required in [
        "SET LOCAL lock_timeout = '5s';",
        "SET LOCAL statement_timeout = '30s';",
        "IN ACCESS EXCLUSIVE MODE;",
        "function_oid OID PRIMARY KEY",
        "function_acl ACLITEM[]",
        "function_row.proacl IS DISTINCT FROM snapshot.function_acl",
        "runtime_execution_slot_writer_epoch_executor_not_quiesced",
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
}

#[test]
fn known_slot_writers_use_the_canonical_physical_lock_order() {
    let postflight = block(MIGRATION, "DO $postflight$", "$postflight$;");
    for required in [
        "global_position < slot_position",
        "slot_position < physical_position",
        "physical_position < deployment_position",
        "replay_position >= begin_position",
        "begin_position >= mutation_position",
        "expected_begin_count",
        "starring_runtime_writer_fence_observe_v1",
        "starring-runtime-serving-slot-v1:",
        "starring_runtime_slot_writer_fence_lock_v2",
        "starring_runtime_slot_writer_fence_begin_unsafe_v2",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
    for (name, replay, mutation, begin_count) in [
        (
            "renew",
            "outcome_name := ''replayed''",
            "UPDATE public.runtime_deployments AS deployment",
            "1::BIGINT",
        ),
        (
            "mutate",
            "outcome_name := ''replayed''",
            "UPDATE public.runtime_deployments AS deployment",
            "1::BIGINT",
        ),
        (
            "prepare",
            "preparation_name := ''replayed''",
            "''::TEXT",
            "0::BIGINT",
        ),
        (
            "commit",
            "outcome_name := ''replayed''",
            "INSERT INTO public.runtime_attestations",
            "1::BIGINT",
        ),
    ] {
        assert!(postflight.contains(&format!("('{name}'")));
        assert!(postflight.contains(replay));
        assert!(postflight.contains(mutation));
        assert!(postflight.contains(begin_count));
    }
}

#[test]
fn renew_mutate_and_certify_place_epoch_changes_at_the_unsafe_boundary() {
    let renew = block(MIGRATION, "DO $patch_renew$", "$patch_renew$;");
    let mutate = block(MIGRATION, "DO $patch_mutate$", "$patch_mutate$;");
    let prepare = block(
        MIGRATION,
        "DO $patch_certify_prepare$",
        "$patch_certify_prepare$;",
    );
    let commit = block(
        MIGRATION,
        "DO $patch_certify_commit$",
        "$patch_certify_commit$;",
    );
    for writer in [renew, mutate, commit] {
        let physical = writer
            .find("starring_runtime_slot_writer_fence_lock_v2")
            .unwrap();
        let begin = writer
            .find("starring_runtime_slot_writer_fence_begin_unsafe_v2")
            .unwrap();
        assert!(physical < begin);
        assert_eq!(
            writer
                .matches("starring_runtime_slot_writer_fence_begin_unsafe_v2")
                .count(),
            1
        );
    }
    let renew_unsafe = &renew[renew.rfind("next_fragment :=").unwrap()..];
    let mutate_unsafe = &mutate[mutate.rfind("next_fragment :=").unwrap()..];
    let commit_unsafe = &commit[commit.rfind("next_fragment :=").unwrap()..];
    assert!(
        renew_unsafe.find("begin_unsafe_v2").unwrap() < renew_unsafe.find("UPDATE public").unwrap()
    );
    assert!(
        mutate_unsafe.find("begin_unsafe_v2").unwrap()
            < mutate_unsafe.find("UPDATE public").unwrap()
    );
    assert!(
        commit_unsafe.find("begin_unsafe_v2").unwrap()
            < commit_unsafe.find("INSERT INTO public").unwrap()
    );
    assert!(prepare.contains("starring_runtime_slot_writer_fence_lock_v2"));
    assert!(!prepare.contains("starring_runtime_slot_writer_fence_begin_unsafe_v2"));
}

#[test]
fn migration_pins_current_and_resulting_contracts_without_new_grants() {
    for digest in [
        "5080cfbe425828c2eb5c54bbe475cba5cf02fa0cecc0aab0a72a4ddb7af5d718",
        "7b49bc478d98af25cdf7563a05d3e03ecddbb7fd2ed897a7bcb2f053716fe386",
        "84ea51ff6db862974303c191d44e41af00685b3384c732b8dbda1ef7a18df08a",
        "ddbda720a96466f784b46de120a199d0b20ac3384a39d70c8829d8747532b105",
        "3c418773f843f2bf8827464624b4fd3124d8979c4c15b4323a96e76676c11c4e",
        "76d965851a753501722854c0aecc22d51a3eaa92e93d55a299bfd59d5d922559",
        "0b9fdc77ec2d85ea2513d6edf462ddddfe3304c4c1f53bec0432b5e0180e6967",
        "5f0fa2982466bf5ca30250d2334c8314fa0c510ae586f6b750cdd1b662655fe1",
        "0d0adb92217032ac62b996a0b3e6cb3cdb3ff99a0be983626aa5df4777c78bb7",
        "b5362bc1b081789a5b3ac4881fc2ea00c340a013630f7d5c809958ed1c045ec3",
    ] {
        assert!(MIGRATION.contains(digest), "{digest}");
    }
    assert!(MIGRATION.contains("00e12af28c93ce77f62c4e1335aa3de88431bb22096bd85b86038dd555dccd13"));
    assert!(!MIGRATION.contains("GRANT EXECUTE"));
    assert!(!MIGRATION.contains("GRANT SELECT"));
    assert!(!MIGRATION.contains("GRANT UPDATE"));
}

#[test]
fn adapter_classifies_pending_drain_as_retry_not_ready_and_pins_readiness() {
    assert!(ERROR_SOURCE
        .contains("\"RX007\" => Some(RuntimeExecutionPersistenceErrorV1::RetryNotReady)"));
    let readiness = "b5362bc1b081789a5b3ac4881fc2ea00c340a013630f7d5c809958ed1c045ec3";
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(readiness));
    }
}
