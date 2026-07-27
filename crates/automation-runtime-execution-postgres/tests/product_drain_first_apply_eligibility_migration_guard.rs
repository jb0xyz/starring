const MIGRATION: &str = include_str!(
    "../../../migrations/202607240011_harden_product_drain_first_apply_eligibility.sql"
);
const FIRST_APPLY_MIGRATION: &str =
    include_str!("../../../migrations/202607240009_add_product_drain_first_apply_core.sql");
const SLOT_FENCE_MIGRATION: &str =
    include_str!("../../../migrations/202607240010_persist_runtime_slot_writer_fence.sql");
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const CORE_IDENTITY: &str = "starring_runtime_private_v2.\
starring_runtime_product_drain_first_apply_core_v2(\
text,text,text,text,text,bigint,text,text,text,text,bigint,text,bigint,text,text,text,\
bytea,text,bytea,text)";
const PREVIOUS_CORE_DIGEST: &str =
    "9668f69cf24d956d4f1f293331c30c81fb46eaea6fcb86e39f05577f02d4c1ac";
const CURRENT_CORE_DIGEST: &str =
    "534dcc1f973d1b37e9f72e28b01ad6541f2ff4293b1cbc5c3b5893764b7fed6e";
const PREVIOUS_MANIFEST_DIGEST: &str =
    "223a7d5a5aba3e418ed310c4cffa8271193af158f12729f74ad85be97123c292";
const CURRENT_MANIFEST_DIGEST: &str =
    "3a014a2c92d5a7da93867f10d8e5d8f9ca1ac5f49666ad57558d49f46b66b2a0";
const PREVIOUS_READINESS_DIGEST: &str =
    "48a10f783603fe02879f2a1cddbecbb39541ac0ca154c77f7b1e0eef8d9f6834";
const CURRENT_READINESS_DIGEST: &str =
    "17fdc258083036bc6f6faceee4dbd900f166ce15f711e99ea87e60ae03e3aa31";
const LATEST_READINESS_DIGEST: &str =
    "de739460f2c86c2016cbc91aa47a625fbced903cc93722de80a33c93c7b54932";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn replacement_for(drift_message: &str) -> &'static str {
    let end = MIGRATION.find(drift_message).unwrap();
    let start = MIGRATION[..end].rfind("previous_fragment :=").unwrap();
    &MIGRATION[start..end]
}

fn next_fragment_for(drift_message: &str) -> &'static str {
    replacement_for(drift_message)
        .split("next_fragment :=")
        .nth(1)
        .unwrap()
}

fn first_apply_body() -> &'static str {
    FIRST_APPLY_MIGRATION
        .split(
            "CREATE FUNCTION \
starring_runtime_private_v2.starring_runtime_product_drain_first_apply_core_v2(",
        )
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn eligibility_migration_is_atomic_comment_free_and_does_not_grow_the_api() {
    assert!(!MIGRATION.contains("--"));
    assert!(!MIGRATION.contains("/*"));
    assert!(!MIGRATION.contains("//"));
    assert!(!MIGRATION.contains("DISABLE TRIGGER"));
    assert!(!MIGRATION.contains("session_replication_role"));
    for forbidden in [
        "CREATE FUNCTION",
        "CREATE OR REPLACE FUNCTION",
        "DROP FUNCTION",
        "ALTER FUNCTION",
        "CREATE TABLE",
        "ALTER TABLE",
        "DROP TABLE",
        "CREATE ROLE",
        "GRANT ",
        "REVOKE ",
        "ALTER DEFAULT PRIVILEGES",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }

    let writer_lock = MIGRATION
        .find("pg_catalog.hashtextextended('starring-runtime-writer-fence-v1', 0)")
        .unwrap();
    let table_lock = MIGRATION.find("LOCK TABLE").unwrap();
    let preflight = MIGRATION.find("DO $preflight$").unwrap();
    let core_patch = MIGRATION.find("DO $patch_core$").unwrap();
    let manifest_patch = MIGRATION.find("DO $patch_manifest$").unwrap();
    let readiness_patch = MIGRATION.find("DO $patch_readiness$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    let reset = MIGRATION.find("RESET search_path;").unwrap();
    assert!(writer_lock < table_lock);
    assert!(table_lock < preflight);
    assert!(preflight < core_patch);
    assert!(core_patch < manifest_patch);
    assert!(manifest_patch < readiness_patch);
    assert!(readiness_patch < postflight);
    assert!(postflight < reset);
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE"));
    for relation in [
        "public.runtime_deployments",
        "public.runtime_serving_leases",
        "public.runtime_product_operations_v2",
        "public.runtime_drain_intents_v2",
        "public.runtime_slot_writer_fences_v2",
    ] {
        assert!(MIGRATION[..preflight].contains(relation), "{relation}");
    }
    assert!(MIGRATION
        .trim_end()
        .ends_with("RESET search_path;\nRESET statement_timeout;\nRESET lock_timeout;"));
}

#[test]
fn preflight_pins_the_complete_predecessor_contract_before_patching() {
    let preflight = dollar_block("preflight");
    for required in [
        CORE_IDENTITY,
        PREVIOUS_CORE_DIGEST,
        PREVIOUS_MANIFEST_DIGEST,
        PREVIOUS_READINESS_DIGEST,
        "public.starring_runtime_interaction_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "5fe0365d0cb4912a01778f3d30a2d649a40e82c5b964ba9e2e7e1901e79eb109",
        "e4bae4b38acc529accd4401af853eb7e96d2a34ad8fb1224b9965166ff40c229",
        "public.starring_runtime_serving_schema_manifest_v1()",
        "14a0c119d8fa0b7a85b72509df29156a6c869b5e3f240bc8fffc89fd1a86c4c9",
        "1c0c79c6fbf528f28fb56e91a54b78cd1fe17c70d2bc3e8d7e3dc515d8a7f8f7",
        "runtime_product_drain_first_apply_eligibility_preflight_drift",
    ] {
        assert!(preflight.contains(required), "{required}");
    }
}

#[test]
fn fresh_apply_locks_the_whole_lane_in_product_apply_order() {
    let patch = dollar_block("patch_core");
    for drift in [
        "runtime_product_drain_first_apply_eligibility_declaration_drift",
        "runtime_product_drain_first_apply_eligibility_lane_lock_drift",
        "runtime_product_drain_first_apply_eligibility_gate_drift",
    ] {
        assert!(patch.contains(drift), "{drift}");
    }
    for declaration in [
        "lane_head_deployment_id TEXT",
        "unresolved_deployment_count BIGINT",
        "unresolved_deployment_id TEXT",
        "serving_row public.runtime_serving_leases%ROWTYPE",
        "serving_found BOOLEAN",
        "eligibility_clock TIMESTAMPTZ",
    ] {
        assert!(patch.contains(declaration), "{declaration}");
    }

    let lane = next_fragment_for("runtime_product_drain_first_apply_eligibility_lane_lock_drift");
    let lane_lock = lane.find("PERFORM deployment.deployment_id").unwrap();
    let slot_guild = lane
        .find("deployment.guild_id = requested_slot_guild_id")
        .unwrap();
    let slot_ruleset = lane
        .find("deployment.ruleset_key = requested_slot_ruleset_key")
        .unwrap();
    let deterministic_order = lane
        .find("ORDER BY deployment.runtime_generation, deployment.deployment_id")
        .unwrap();
    let row_lock = lane.find("FOR UPDATE").unwrap();
    let requested_deployment = lane.find("SELECT deployment.*").unwrap();
    assert!(lane_lock < slot_guild);
    assert!(slot_guild < slot_ruleset);
    assert!(slot_ruleset < deterministic_order);
    assert!(deterministic_order < row_lock);
    assert!(row_lock < requested_deployment);
    assert!(!lane[lane_lock..requested_deployment].contains("deployment.phase"));
    assert!(SLOT_FENCE_MIGRATION.contains("starring_runtime_slot_writer_fence_lock_v2"));
    assert!(
        MIGRATION.find("eligibility_lane_lock_drift").unwrap()
            < MIGRATION.find("eligibility_gate_drift").unwrap()
    );
}

#[test]
fn exact_replay_returns_before_every_fresh_eligibility_check() {
    let base = first_apply_body();
    let replay = base.find("outcome_name := 'replayed'").unwrap();
    let replay_return = base[replay..].find("RETURN;").unwrap() + replay;
    let fresh_anchor = base
        .find(
            "PERFORM 1\n    FROM public.runtime_product_operations_v2 AS product\n    \
WHERE product.product_operation_id = requested_operation_id",
        )
        .unwrap();
    assert!(replay < replay_return);
    assert!(replay_return < fresh_anchor);
    assert!(base.contains("deployment_row.phase NOT IN ('awaiting_gateway_ready', 'live')"));
    assert!(SLOT_FENCE_MIGRATION.contains("slot_fence_row.pending_drain_intent_id IS NOT NULL"));
    assert!(SLOT_FENCE_MIGRATION.contains("outcome_name := ''slot_conflict''"));

    let replacement = replacement_for("runtime_product_drain_first_apply_eligibility_gate_drift");
    assert!(replacement.contains(
        "'    PERFORM 1' || E'\\n' ||\n        \
'    FROM public.runtime_product_operations_v2 AS product'"
    ));
    assert!(
        !next_fragment_for("runtime_product_drain_first_apply_eligibility_gate_drift")
            .contains("outcome_name")
    );
    assert!(dollar_block("preflight").contains(PREVIOUS_CORE_DIGEST));
}

#[test]
fn fresh_gate_uses_one_locked_snapshot_and_a_newest_nonretired_head() {
    let gate = next_fragment_for("runtime_product_drain_first_apply_eligibility_gate_drift");
    let head = gate.find("SELECT deployment.deployment_id").unwrap();
    let unresolved = gate.find("SELECT pg_catalog.count(*)").unwrap();
    let serving = gate.find("SELECT lease.*").unwrap();
    let serving_lock = gate[serving..].find("FOR UPDATE").unwrap() + serving;
    let found = gate.find("serving_found := FOUND").unwrap();
    let clock = gate
        .find("eligibility_clock := pg_catalog.clock_timestamp()")
        .unwrap();
    let eligibility = gate
        .find("IF lane_head_deployment_id IS DISTINCT FROM requested_deployment_id")
        .unwrap();
    let identifier_lookup = gate
        .find("FROM public.runtime_product_operations_v2 AS product")
        .unwrap();
    assert!(head < unresolved);
    assert!(unresolved < serving);
    assert!(serving < serving_lock);
    assert!(serving_lock < found);
    assert!(found < clock);
    assert!(clock < eligibility);
    assert!(eligibility < identifier_lookup);
    for required in [
        "deployment.phase NOT IN (''superseded'', ''cancelled'')",
        "ORDER BY deployment.runtime_generation DESC, deployment.deployment_id DESC",
        "LIMIT 1",
        "deployment.phase NOT IN (''live'', ''superseded'', ''cancelled'')",
        "lane_head_deployment_id IS DISTINCT FROM requested_deployment_id",
        "runtime_product_drain_first_apply_deployment_mismatch",
        "ERRCODE = ''RX001''",
    ] {
        assert!(gate.contains(required), "{required}");
    }
}

#[test]
fn awaiting_gateway_ready_allows_only_a_single_unresolved_head_without_a_fresh_occupant() {
    let gate = next_fragment_for("runtime_product_drain_first_apply_eligibility_gate_drift");
    let awaiting_start = gate
        .find("deployment_row.phase = ''awaiting_gateway_ready''")
        .unwrap();
    let live_start = gate.find("deployment_row.phase = ''live''").unwrap();
    let awaiting = &gate[awaiting_start..live_start];
    for required in [
        "unresolved_deployment_count IS DISTINCT FROM 1",
        "unresolved_deployment_id",
        "IS DISTINCT FROM requested_deployment_id",
        "serving_found",
        "serving_row.connected",
        "serving_row.serving",
        "serving_row.expires_at > eligibility_clock",
    ] {
        assert!(awaiting.contains(required), "{required}");
    }
    for forbidden in [
        "serving_row.tenant_id",
        "serving_row.installation_id",
        "serving_row.deployment_id",
        "serving_row.attestation_id",
    ] {
        assert!(!awaiting.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn live_requires_the_exact_serving_identity_but_accepts_expired_or_disconnected_occupants() {
    let gate = next_fragment_for("runtime_product_drain_first_apply_eligibility_gate_drift");
    let live_start = gate.find("deployment_row.phase = ''live''").unwrap();
    let gate_end = gate[live_start..].find("'    THEN'").unwrap() + live_start;
    let live = &gate[live_start..gate_end];
    for required in [
        "unresolved_deployment_count IS DISTINCT FROM 0",
        "NOT serving_found",
        "serving_row.tenant_id",
        "deployment_row.tenant_id",
        "serving_row.installation_id",
        "deployment_row.installation_id",
        "serving_row.deployment_id",
        "deployment_row.deployment_id",
        "serving_row.attestation_id",
        "deployment_row.live_attestation_id",
        "serving_row.runtime_generation",
        "deployment_row.runtime_generation",
        "serving_row.guild_id",
        "deployment_row.guild_id",
        "serving_row.ruleset_key",
        "deployment_row.ruleset_key",
        "serving_row.target_version",
        "deployment_row.target_version",
        "serving_row.target_content_hash",
        "deployment_row.target_content_hash",
        "serving_row.binding_revision",
        "deployment_row.binding_revision",
        "serving_row.binding_fingerprint",
        "deployment_row.binding_fingerprint",
    ] {
        assert!(live.contains(required), "{required}");
    }
    for forbidden in [
        "serving_row.connected",
        "serving_row.serving",
        "serving_row.expires_at",
        "eligibility_clock",
    ] {
        assert!(!live.contains(forbidden), "{forbidden}");
    }
    assert_eq!(live.matches("IS DISTINCT FROM").count(), 12);
}

#[test]
fn manifest_readiness_postflight_and_rust_pins_move_as_one_contract() {
    let manifest = dollar_block("patch_manifest");
    let readiness = dollar_block("patch_readiness");
    let postflight = dollar_block("postflight");
    for required in [
        "RETURN observed_count = 623",
        "ce1e493041abc52b6f4073da976a99b547b32a92d7ff171b64eef791354ff491",
        "68588695f6c82923f7830faa333d16533f86b43f3f47bf69756bd7447c1aae91",
        "runtime_product_drain_first_apply_eligibility_manifest_drift",
    ] {
        assert!(manifest.contains(required), "{required}");
    }
    for required in [
        PREVIOUS_MANIFEST_DIGEST,
        CURRENT_MANIFEST_DIGEST,
        "runtime_product_drain_first_apply_eligibility_readiness_drift",
    ] {
        assert!(readiness.contains(required), "{required}");
    }
    for required in [
        CORE_IDENTITY,
        CURRENT_CORE_DIGEST,
        CURRENT_MANIFEST_DIGEST,
        CURRENT_READINESS_DIGEST,
        "ORDER BY deployment.runtime_generation, deployment.deployment_id",
        "eligibility_clock := pg_catalog.clock_timestamp()",
        "runtime_product_drain_first_apply_eligibility_postflight_drift",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DIGEST));
        assert!(!source.contains(CURRENT_READINESS_DIGEST));
        assert!(!source.contains(PREVIOUS_READINESS_DIGEST));
    }
    for placeholder in [
        "__CORE",
        "__MANIFEST",
        "__READINESS",
        "__POSTFLIGHT",
        "__DIGEST",
    ] {
        assert!(!MIGRATION.contains(placeholder), "{placeholder}");
    }
}
