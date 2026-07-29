const MIGRATION: &str =
    include_str!("../../../migrations/202607280005_cancel_runtime_product_drain_v2.sql");

#[test]
fn lifecycle_cancellation_migration_is_atomic_replayable_and_acl_sealed() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    for required in [
        "CREATE FUNCTION public.starring_product_cancel_runtime_drain_v2(",
        "expected_cancellation_reason_digest TEXT",
        "expected_drain_intent_id TEXT",
        "expected_source_intent_revision BIGINT",
        "expected_source_state_digest TEXT",
        "expected_product_operation_id TEXT",
        "expected_source_deployment_revision BIGINT",
        "source_product_mutation_request_bytes BYTEA",
        "source_drain_intent_request_bytes BYTEA",
        "source_deployment_snapshot JSONB",
        "source_result_deployment_snapshot JSONB",
        "source_state_bytes BYTEA",
        "result_state_bytes BYTEA",
        "terminal_projection_bytes BYTEA",
        "source_deployment_snapshot_bytes BYTEA",
        "source_deployment_snapshot_digest TEXT",
        "source_canonical_state_bytes BYTEA",
        "starring_product_apply_consume_preparation_reservation_v2(",
        "'prepare'",
        "'commit'",
        "starring_runtime_product_drain_cancel_source_v2(",
        "starring_runtime_product_drain_terminal_transition_v2(",
        "starring_runtime_slot_writer_fence_terminal_release_v2(",
        "starring_runtime_product_drain_cancelled_terminal_exact_v2(",
        "INSERT INTO public.runtime_product_drain_terminal_actions_v2",
        "action_row.source_deployment_snapshot_bytes",
        "action_row.source_canonical_state_bytes",
        "IF drain_row.intent_state IN ('consumed', 'cancelled') THEN",
        "outcome_name := 'replayed';",
        "outcome_name := 'applied';",
        "DO $seal_product_lifecycle_cancellation_acl_final$",
        "REVOKE ALL ON FUNCTION %s FROM PUBLIC",
        "DO $postflight$",
        "RETURN observed_count = 911",
        "ae39639ca7f4f2d911e227b8429d1566efdc677dbfd641d8fcf5f24d376baf8b",
        "b7ee8d2a13ae38a88bc1b2558b018e74893e7d90ccd72d96187197a111432e22",
        "3fe2924d130e93d630960be796e3986884fefedddfb91c0dd5b680a41b440cb1",
    ] {
        assert!(
            MIGRATION.contains(required),
            "missing lifecycle cancellation migration guard: {required}"
        );
    }
    for forbidden in [
        "DELETE FROM public.runtime_product_drain_terminal_actions_v2",
        "UPDATE public.runtime_product_drain_terminal_actions_v2",
        "TRUNCATE public.runtime_product_drain_terminal_actions_v2",
        "GRANT EXECUTE ON FUNCTION public.starring_product_cancel_runtime_drain_v2",
    ] {
        assert!(
            !MIGRATION.contains(forbidden),
            "forbidden lifecycle cancellation migration edge: {forbidden}"
        );
    }
}

#[test]
fn lifecycle_cancellation_public_result_contract_remains_fixed() {
    let function = MIGRATION
        .split("CREATE FUNCTION public.starring_product_cancel_runtime_drain_v2(")
        .nth(1)
        .unwrap()
        .split("DO $create_product_lifecycle_cancellation_record$")
        .next()
        .unwrap();
    let result = function
        .split("RETURNS TABLE(")
        .nth(1)
        .unwrap()
        .split(")\nLANGUAGE plpgsql")
        .next()
        .unwrap();
    let expected = [
        "outcome_name TEXT",
        "exact_replay BOOLEAN",
        "product_resulting_revision BIGINT",
        "product_resulting_state TEXT",
        "guild_id TEXT",
        "product_receipt_id TEXT",
        "product_audit_event_id TEXT",
        "cancellation_reason_digest TEXT",
        "product_operation_id TEXT",
        "source_product_mutation_request_bytes BYTEA",
        "product_mutation_digest TEXT",
        "source_drain_intent_request_bytes BYTEA",
        "drain_intent_digest TEXT",
        "source_deployment_id TEXT",
        "source_deployment_revision BIGINT",
        "source_deployment_snapshot JSONB",
        "source_deployment_snapshot_digest TEXT",
        "source_result_deployment_revision BIGINT",
        "source_result_deployment_snapshot JSONB",
        "source_result_deployment_snapshot_digest TEXT",
        "drain_intent_id TEXT",
        "source_intent_revision BIGINT",
        "source_state_bytes BYTEA",
        "source_state_digest TEXT",
        "result_intent_revision BIGINT",
        "result_intent_state TEXT",
        "result_state_bytes BYTEA",
        "result_state_digest TEXT",
        "source_slot_epoch BIGINT",
        "successor_slot_epoch BIGINT",
        "terminal_action_id TEXT",
        "terminal_projection_bytes BYTEA",
        "terminal_projection_digest TEXT",
        "terminal_database_time TIMESTAMPTZ",
    ];
    let observed = result
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_end_matches(','))
        .collect::<Vec<_>>();
    assert_eq!(observed, expected);
}
