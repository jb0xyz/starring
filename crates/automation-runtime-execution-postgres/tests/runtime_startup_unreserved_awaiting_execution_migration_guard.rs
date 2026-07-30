const MIGRATION: &str =
    include_str!("../../../migrations/202607310007_recover_unreserved_awaiting_gateway_v2.sql");
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607310006_refresh_runtime_certification_readiness_pin.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");

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
fn migration_is_forward_only_comment_free_and_manifest_pinned() {
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET lock_timeout;\n"));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    assert!(MIGRATION
        .contains("starring_runtime_private_v2.starring_runtime_startup_unreserved_execute_v2"));
    assert!(MIGRATION.contains(
        "starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2"
    ));
    assert!(dollar_block("postflight").contains(
        "'starring_runtime_private_v2.starring_runtime_startup_unreserved_projection_exact_v2(bytea,timestamp with time zone)',\n                'v'::\"char\""
    ));
    assert!(!PREVIOUS_MIGRATION.contains("startup_unreserved"));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    let manifest = dollar_block("refresh_manifest");
    assert!(manifest.contains("ec41d06fbdfce734b673f6e4e7864e428fb153af992c4f3c395a0eb1cd2106a4"));
    assert!(manifest.contains("dd7a64d16d27a32dde6f80416e4efc444c69aa59e055ff26f8008a2cdc845a62"));
    let readiness = dollar_block("refresh_readiness");
    assert!(readiness.contains("6731f361eb37f170d4cdb91a1c5931101ef6bc2d16c50e1114a452e05b228f7b"));
    assert!(readiness.contains("ee35572e966037477a9070fef87781e901f0ef49e3cb471acebba9c165657676"));
    assert!(MIGRATION.contains("437eef0962f31be61e9fcb2f6705b2cda14f4d52105ae024ca4bc29b967e001c"));
    assert!(CONTRACT_SOURCE
        .contains("public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2"));
    assert!(CONTRACT_SOURCE
        .contains("437eef0962f31be61e9fcb2f6705b2cda14f4d52105ae024ca4bc29b967e001c"));
}

#[test]
fn observation_selects_only_exact_unreserved_awaiting_scopes() {
    let patch = dollar_block("patch_observation");
    for required in [
        "unreserved_awaiting_count",
        "executable_unreserved_awaiting_count",
        "blocked_unreserved_awaiting_count",
        "deployment.phase = ''awaiting_gateway_ready''",
        "deployment.snapshot -> ''panel_certificate''",
        "runtime_certification_operations_v2",
        "route_absent_acknowledged",
        "recoverable_awaiting_certification_count",
    ] {
        assert!(patch.contains(required), "{required}");
    }
}

#[test]
fn execution_preserves_owner_action_slot_and_product_fences() {
    for required in [
        "starring-runtime-serving-slot-v1:",
        "runtime_gateway_owners",
        "runtime_startup_recovery_actions_v2",
        "FOR UPDATE",
        "starring_runtime_slot_writer_fence_begin_unsafe_v2",
        "starring_runtime_lock_current_authority",
        "runtime_drain_intents_v2",
        "route_absent_acknowledged",
        "runtime_attestations",
        "runtime_serving_leases",
        "starring_runtime_cert_awaiting_reset_exact_v2",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    let slot = MIGRATION.find("starring-runtime-serving-slot-v1:").unwrap();
    let transition = MIGRATION
        .find("UPDATE public.runtime_deployments AS deployment")
        .unwrap();
    let journal = MIGRATION
        .rfind("starring_runtime_startup_recovery_action_record_v2")
        .unwrap();
    assert!(slot < transition && transition < journal);
}

#[test]
fn unreserved_projection_is_distinct_exact_and_replayable() {
    assert!(!MIGRATION.contains("int8recv"));
    for required in [
        "pg_catalog.int2send(2::SMALLINT)",
        "FOR frame_index IN 1..4 LOOP",
        "pg_catalog.jsonb_send(source_deployment_json)",
        "pg_catalog.to_jsonb(source_deployment)",
        "source_slot.writer_epoch",
        "successor_slot.writer_epoch",
        "current_deployment.revision >",
        "starring_runtime_startup_unreserved_projection_exact_v2",
        "unreserved_progressed_projection_prefix",
        "terminal_outcome_name := ''progressed''",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
}
