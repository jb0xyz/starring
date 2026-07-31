use std::fs;

const MIGRATION_NAME: &str = "202607280003_add_runtime_product_drain_terminal_substrate_v2.sql";
const PREVIOUS_MIGRATION_NAME: &str = "202607280002_compose_product_apply_runtime_drain_v2.sql";
const MIGRATION: &str = include_str!(
    "../../../migrations/202607280003_add_runtime_product_drain_terminal_substrate_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");

fn function(name: &str) -> &'static str {
    MIGRATION
        .split(&format!("CREATE FUNCTION {name}("))
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn migration_is_ordered_bounded_and_comment_free() {
    let migration_directory = format!("{}/../../migrations", env!("CARGO_MANIFEST_DIR"));
    let mut migrations = fs::read_dir(migration_directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".sql"))
        .collect::<Vec<_>>();
    migrations.sort();
    let previous = migrations
        .iter()
        .position(|name| name == PREVIOUS_MIGRATION_NAME)
        .unwrap();
    let current = migrations
        .iter()
        .position(|name| name == MIGRATION_NAME)
        .unwrap();
    assert_eq!(current, previous + 1);
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(
        MIGRATION.ends_with("RESET search_path;\nRESET statement_timeout;\nRESET lock_timeout;\n")
    );
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"), "{line}");
        assert!(!trimmed.starts_with("//"), "{line}");
        assert!(!trimmed.starts_with("/*"), "{line}");
    }
    assert!(MIGRATION.contains("pg_advisory_xact_lock("));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    assert!(!MIGRATION.contains("SKIP LOCKED"));
}

#[test]
fn terminal_journal_has_closed_kind_specific_deployment_shape() {
    let table = MIGRATION
        .split("CREATE TABLE public.runtime_product_drain_terminal_actions_v2 (")
        .nth(1)
        .unwrap()
        .split("\n);")
        .next()
        .unwrap();
    for required in [
        "source_deployment_revision BIGINT NOT NULL",
        "source_result_deployment_revision BIGINT NOT NULL",
        "source_result_deployment_snapshot_digest TEXT NOT NULL",
        "result_deployment_id TEXT",
        "result_deployment_revision BIGINT",
        "result_deployment_snapshot_digest TEXT",
        "source_result_deployment_revision =\n                source_deployment_revision + 1",
        "terminal_kind = 'consumed'",
        "result_deployment_id IS NOT NULL",
        "result_deployment_id\n                        ~ '^[0-9a-f]{32}$'",
        "result_deployment_revision IS NOT NULL",
        "result_deployment_revision = 1",
        "result_deployment_snapshot_digest IS NOT NULL",
        "terminal_kind = 'cancelled'",
        "cancellation_reason_digest IS NOT NULL",
        "result_deployment_id IS NULL",
        "result_deployment_revision IS NULL",
        "result_deployment_snapshot_digest IS NULL",
        "successor_slot_writer_epoch =\n                source_slot_writer_epoch + 1",
    ] {
        assert!(table.contains(required), "{required}");
    }
    for required in [
        "runtime_product_drain_terminal_actions_v2_drain_unique",
        "runtime_product_drain_terminal_actions_v2_action_unique",
        "runtime_product_drain_terminal_actions_v2_product_fk",
        "runtime_product_drain_terminal_actions_v2_drain_fk",
        "terminal_projection_digest =",
        "pg_catalog.sha256(terminal_projection_bytes)",
    ] {
        assert!(table.contains(required), "{required}");
    }
}

#[test]
fn terminal_canonical_builders_and_projection_are_exact() {
    let consumed =
        function("starring_runtime_private_v2.starring_runtime_pending_drain_consumed_state_v2");
    let cancelled =
        function("starring_runtime_private_v2.starring_runtime_pending_drain_cancelled_state_v2");
    let projection = function(
        "starring_runtime_private_v2.starring_runtime_product_drain_terminal_projection_v2",
    );
    for body in [consumed, cancelled] {
        for required in [
            "IMMUTABLE",
            "STRICT",
            "PARALLEL SAFE",
            "SECURITY INVOKER",
            "SET search_path = pg_catalog",
            "source_row.intent_state <> 'route_absent_acknowledged'",
            "starring_runtime_pending_drain_state_exact_v2(",
            "source_row.intent_revision + 1",
            "terminal_microseconds::BIGINT::TEXT",
        ] {
            assert!(body.contains(required), "{required}");
        }
    }
    assert!(consumed.contains("\"state\":{\"kind\":\"consumed\",\"resulting_revision\":"));
    assert!(consumed.contains("requested_resulting_revision <> 1"));
    assert!(cancelled.contains("\"state\":{\"kind\":\"cancelled\","));
    for required in [
        "requested_source_result_deployment_revision",
        "requested_source_result_deployment_snapshot_digest",
        "requested_result_deployment_id",
        "COALESCE(requested_result_deployment_revision, 0)",
        "requested_terminal_kind = 'consumed'",
        "requested_result_deployment_revision <> 1",
        "requested_terminal_kind = 'cancelled'",
        "starring.runtime.product_drain.terminal.v2",
        "pg_catalog.decode('00', 'hex')",
        "pg_catalog.sha256(payload_bytes)",
    ] {
        assert!(projection.contains(required), "{required}");
    }
    assert!(!projection.contains(
        "requested_result_deployment_revision\n            <> requested_source_deployment_revision + 1"
    ));
}

#[test]
fn terminal_exact_validator_distinguishes_source_and_new_deployment() {
    let validator = function(
        "starring_runtime_private_v2.starring_runtime_product_drain_terminal_action_exact_v2",
    );
    for required in [
        "product_row.expected_revision\n            <> action_row.source_deployment_revision",
        "action_row.result_deployment_id =\n                    drain_row.deployment_id",
        "state,resulting_revision",
        "action_row.result_deployment_revision",
        "action_row.result_deployment_revision\n                    IS DISTINCT FROM 1",
        "action_row.source_result_deployment_revision",
        "action_row.source_result_deployment_snapshot_digest",
        "action_row.result_deployment_id",
        "starring_runtime_product_drain_terminal_projection_v2(",
    ] {
        assert!(validator.contains(required), "{required}");
    }
}

#[test]
fn mutation_gates_release_exact_epoch_and_clear_complete_fence() {
    let transition = function(
        "starring_runtime_private_v2.starring_runtime_product_drain_terminal_transition_v2",
    );
    let release = function(
        "starring_runtime_private_v2.starring_runtime_slot_writer_fence_terminal_release_v2",
    );
    for required in [
        "source_row.intent_state\n            <> 'route_absent_acknowledged'",
        "NOT IN ('consumed', 'cancelled')",
        "'terminal_update'",
        "source_row.intent_revision + 1",
        "canonical_state_digest =",
        "requested_resulting_deployment_revision <> 1",
        "runtime_product_drain_terminal_transition_causal_clock_invalid",
        "{state,acknowledgement,acknowledged_at_unix_microseconds}",
    ] {
        assert!(transition.contains(required), "{required}");
    }
    for required in [
        "terminal_release",
        "SET writer_epoch = successor_epoch",
        "pending_drain_intent_id = NULL",
        "pending_product_operation_id = NULL",
        "pending_tenant_id = NULL",
        "pending_installation_id = NULL",
        "pending_deployment_id = NULL",
        "pending_expected_revision = NULL",
        "pending_marked_at = NULL",
        "updated_at = requested_terminal_time",
        "runtime_slot_writer_fence_terminal_release_causal_clock_invalid",
        "{state,acknowledgement,acknowledged_at_unix_microseconds}",
    ] {
        assert!(release.contains(required), "{required}");
    }
    assert!(release.contains("runtime_slot_writer_fence_terminal_release_gate_invalid"));
    assert!(MIGRATION.contains("validate_runtime_slot_writer_fence_symmetry_v2"));
    assert!(MIGRATION.contains("action.drain_intent_id = drain.drain_intent_id"));
}

#[test]
fn journal_is_append_only_private_and_no_public_terminal_capability_exists() {
    for required in [
        "BEFORE UPDATE OR DELETE",
        "BEFORE TRUNCATE",
        "runtime_product_drain_terminal_action_mutation_rejected",
        "REVOKE ALL PRIVILEGES\nON TABLE public.runtime_product_drain_terminal_actions_v2\nFROM PUBLIC",
        "REVOKE ALL PRIVILEGES ON FUNCTION",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION
        .contains("CREATE FUNCTION public.starring_product_apply_consume_runtime_drain_v2"));
    assert!(!MIGRATION.contains("CREATE FUNCTION public.starring_product_cancel_runtime_drain_v2"));
    assert!(MIGRATION.contains("invalid_public_capability_count"));
}

#[test]
fn readers_and_manifests_accept_terminal_history_without_selecting_it() {
    for required in [
        "''pending''::text, ''route_absent_acknowledged''::text, ''consumed''::text, ''cancelled''::text",
        "drain_row.intent_state IN (",
        "runtime_product_drain_terminal_actions_v2",
        "starring_runtime_product_drain_terminal_action_exact_v2(",
        "RETURN observed_count = 888",
        "aacb4889c005088a91b93ee948502397aa8747275087a4e2a600d2d49a9b8181",
        "0e40c195026bf46ce6a8e5e70472d108de5deb533d1f072cf056e171c7078fe7",
        "a3674e7c69f24ce212ddf0598d23f448a47f0b6e7766dee20a78399d5b6477e7",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(CONTRACT_SOURCE
        .contains("0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f"));
    assert!(MIGRATION.contains(
        "CREATE UNIQUE INDEX runtime_drain_intents_v2_one_pending_per_slot ON public.runtime_drain_intents_v2 USING btree (slot_guild_id, slot_ruleset_key) WHERE (intent_state = ANY (ARRAY[''pending''::text, ''route_absent_acknowledged''::text]))"
    ));
}
