const MIGRATION: &str =
    include_str!("../../../migrations/202607240012_fence_product_apply_slot_writer_epoch.sql");

fn block<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).unwrap();
    let end = source[start..].find(end).unwrap() + start + end.len();
    &source[start..end]
}

#[test]
fn migration_is_atomic_bounded_and_comment_free() {
    assert!(MIGRATION.contains("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.contains("SET LOCAL statement_timeout = '30s';"));
    assert!(MIGRATION.contains("pg_advisory_xact_lock("));
    assert!(MIGRATION.contains("public.runtime_slot_writer_fences_v2,"));
    assert!(MIGRATION.contains("public.runtime_drain_intents_v2,"));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    assert!(MIGRATION.contains("RESET statement_timeout;"));
    assert!(MIGRATION.contains("RESET lock_timeout;"));
    assert!(MIGRATION.contains("RESET search_path;"));
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
}

#[test]
fn wrapper_locks_the_physical_fence_before_lane_rows_without_advancing_it() {
    let patch = block(MIGRATION, "DO $patch_wrapper$", "$patch_wrapper$;");
    let next = block(
        patch,
        "next_physical_lock :=",
        "previous_delegation_start :=",
    );
    let physical = next
        .find("starring_runtime_slot_writer_fence_lock_v2")
        .unwrap();
    let lane = next.find("FROM public.runtime_deployments").unwrap();
    assert!(physical < lane);
    assert!(!next.contains("starring_runtime_slot_writer_fence_begin_unsafe_v2"));
    assert!(patch.contains("serving_slot_guild_id"));
    assert!(patch.contains("serving_slot_ruleset_key"));
    assert!(patch.contains("slot_pending_drain_intent_id TEXT;"));
}

#[test]
fn wrapper_converts_only_ready_with_pending_and_passes_replays_through() {
    let patch = block(MIGRATION, "DO $patch_wrapper$", "$patch_wrapper$;");
    let next = block(patch, "next_delegation_end :=", "IF pg_catalog.strpos");
    let ready = next.find("core_row.outcome = ''ready''").unwrap();
    let pending = next
        .find("slot_pending_drain_intent_id IS NOT NULL")
        .unwrap();
    let drain = next.find("''runtime_drain_required''").unwrap();
    let replay = next.find("core_row.exact_replay").unwrap();
    assert!(ready < pending);
    assert!(pending < drain);
    assert!(drain < replay);
    assert!(next.contains("core_row.locked_projection"));
}

#[test]
fn finalizer_advances_exactly_once_after_revalidation_and_before_mutation() {
    let patch = block(MIGRATION, "DO $patch_finalize$", "$patch_finalize$;");
    let next = block(patch, "next_mutation :=", "IF pg_catalog.strpos");
    let physical = next
        .find("starring_runtime_slot_writer_fence_lock_v2")
        .unwrap();
    let begin = next
        .find("starring_runtime_slot_writer_fence_begin_unsafe_v2")
        .unwrap();
    let mutation = next
        .find("starring.product_approval_context_digest")
        .unwrap();
    assert!(physical < begin);
    assert!(begin < mutation);
    assert_eq!(
        next.matches("starring_runtime_slot_writer_fence_begin_unsafe_v2")
            .count(),
        1
    );
    assert!(patch.contains("slot_writer_epoch BIGINT;"));
}

#[test]
fn postflight_pins_replay_and_canonical_lock_order() {
    let postflight = block(MIGRATION, "DO $postflight$", "$postflight$;");
    for required in [
        "wrapper_global_position < wrapper_global_row_position",
        "wrapper_global_row_position < wrapper_global_share_position",
        "wrapper_global_share_position < wrapper_slot_position",
        "wrapper_slot_position < wrapper_physical_position",
        "wrapper_physical_position < wrapper_deployment_position",
        "wrapper_deployment_position < wrapper_delegate_position",
        "wrapper_delegate_position < wrapper_ready_position",
        "wrapper_ready_position < wrapper_pending_position",
        "wrapper_pending_position < wrapper_drain_required_position",
        "wrapper_drain_required_position < wrapper_passthrough_position",
        "finalizer_lock_position < finalizer_replay_position",
        "finalizer_replay_position < finalizer_projection_position",
        "finalizer_projection_position < finalizer_physical_position",
        "finalizer_physical_position < finalizer_begin_position",
        "finalizer_begin_position < finalizer_activation_position",
        "finalizer_activation_position < finalizer_runtime_position",
    ] {
        assert!(postflight.contains(required));
    }
    assert!(postflight.contains("lock_row.exact_replay"));
    assert!(postflight.contains("starring_product_apply_lock_core_unfenced_v1"));
}

#[test]
fn migration_preserves_public_contracts_and_private_helper_boundary() {
    assert!(MIGRATION.contains("pg_temp.starring_product_apply_slot_epoch_snapshot"));
    assert!(MIGRATION.contains("function_row.proowner IS DISTINCT FROM snapshot.function_owner"));
    assert!(MIGRATION.contains("function_row.proacl IS DISTINCT FROM snapshot.function_acl"));
    assert!(
        MIGRATION.contains("'68708aa143de1daae1247b18f3127e2abdc6d269a14e103d24e5ab6732d23f99'")
    );
    assert!(
        MIGRATION.contains("'da6c88ff80cf366e14f2c12a6204964d708156192a292cc6ad71b959588f07b8'")
    );
    assert!(!MIGRATION.contains("GRANT EXECUTE"));
    assert!(!MIGRATION.contains("GRANT SELECT"));
    assert!(!MIGRATION.contains("GRANT UPDATE"));
}
