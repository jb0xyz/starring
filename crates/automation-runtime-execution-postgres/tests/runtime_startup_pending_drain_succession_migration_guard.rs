use std::fs;

const MIGRATION_NAME: &str = "202607280001_add_pending_drain_succession_v3.sql";
const PREVIOUS_MIGRATION_NAME: &str =
    "202607270012_add_owner_fenced_startup_pending_drain_execution_v2.sql";
const MIGRATION: &str =
    include_str!("../../../migrations/202607280001_add_pending_drain_succession_v3.sql");
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607270012_add_owner_fenced_startup_pending_drain_execution_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");

const SELECTOR: &str = "public.starring_runtime_startup_recovery_select_pending_drain_v3";
const SUCCESSION: &str = "public.starring_runtime_startup_recovery_pending_drain_succession_v3";
const PREDECESSOR_EXACT: &str =
    "starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3";
const SUCCESSOR_EXACT: &str =
    "starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3";
const PROJECTION: &str =
    "starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3";
const PROJECTION_FRAME: &str =
    "starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3";
const SELECTOR_IDENTITY: &str = "public.starring_runtime_startup_recovery_select_pending_drain_v3(text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone)";
const SUCCESSION_IDENTITY: &str = "public.starring_runtime_startup_recovery_pending_drain_succession_v3(text,bigint,bigint,bigint,bigint,text,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,text,bigint,bigint,text,bigint,bigint,bigint,bigint,text,bigint,bigint,bigint,text,bigint,text,text,boolean,bigint,bigint,bytea,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,bigint,boolean)";
const PREDECESSOR_IDENTITY: &str = "starring_runtime_private_v2.starring_runtime_pending_drain_predecessor_exact_v3(public.runtime_drain_intents_v2,public.runtime_startup_recovery_actions_v2)";
const SUCCESSOR_IDENTITY: &str = "starring_runtime_private_v2.starring_runtime_pending_drain_successor_exact_v3(public.runtime_drain_intents_v2,public.runtime_drain_intents_v2,public.runtime_deployments,public.runtime_deployments,bytea,text)";
const PROJECTION_IDENTITY: &str = "starring_runtime_private_v2.starring_runtime_pending_drain_succession_projection_v3(bytea,bytea,bytea,bytea)";
const PROJECTION_FRAME_IDENTITY: &str = "starring_runtime_private_v2.starring_runtime_pending_drain_projection_frame_v3(bytea,smallint,smallint)";
const MANIFEST_DEFINITION_DIGEST: &str =
    "8f62326b250fba74273b2dbbf33066ef7f1353e9a6f3f464c059b1678bb714d4";
const READINESS_DEFINITION_DIGEST: &str =
    "d73ca3b8f02623884ccf1e77390395a1daeee1d5c3d12274f865740d0798fa06";
const MANIFEST_OBSERVED_DIGEST: &str =
    "90d1ab7064fa288e01b09e81815265d82409ceac50267412ff952f63a6c285a3";
const SELECTOR_DEFINITION_DIGEST: &str =
    "67ce81c4a3dcb38936eb52872f5a60cddd16936d5ef7eb7599141a3e86f23975";
const SUCCESSION_DEFINITION_DIGEST: &str =
    "c6c3642cad780abea816e0f05a183c7fb9af7376e7379a077b4b2343012cae23";
const PREDECESSOR_DEFINITION_DIGEST: &str =
    "05f495a38a16a2ab0ce057f6e1367fb8510ea95795e14f986b3b806b7b266c8e";
const SUCCESSOR_DEFINITION_DIGEST: &str =
    "2d384bb36f84ae6e4ae64ccc9ef435692ee7e7013bb81de20f572be7bf41c9ca";
const PROJECTION_DEFINITION_DIGEST: &str =
    "1196a9589f25a699946dcec4f937516f72b1b31549ca28ebfaeaa001ce4ce189";
const PROJECTION_FRAME_DEFINITION_DIGEST: &str =
    "cd7223978da9cde002eb693fd276ad79849991c66df25697a55c61d9453d28e2";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn function(name: &str) -> &'static str {
    MIGRATION
        .split(&format!("CREATE FUNCTION {name}("))
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

fn return_fields(name: &str) -> Vec<&'static str> {
    function(name)
        .split("RETURNS TABLE(")
        .nth(1)
        .unwrap()
        .split(")\nLANGUAGE")
        .next()
        .unwrap()
        .lines()
        .map(str::trim)
        .map(|line| line.trim_end_matches(','))
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn migration_is_ordered_collision_safe_bounded_and_comment_free() {
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
    assert!(MIGRATION.ends_with("RESET search_path;\n"));
    for identity in [
        SELECTOR_IDENTITY,
        SUCCESSION_IDENTITY,
        PREDECESSOR_IDENTITY,
        SUCCESSOR_IDENTITY,
        PROJECTION_IDENTITY,
        PROJECTION_FRAME_IDENTITY,
    ] {
        assert!(!PREVIOUS_MIGRATION.contains(identity), "{identity}");
        assert!(dollar_block("preflight").contains(identity), "{identity}");
    }
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"), "{line}");
        assert!(!trimmed.starts_with("//"), "{line}");
        assert!(!trimmed.starts_with("/*"), "{line}");
        if let Some(qualified) = trimmed.strip_prefix("CREATE FUNCTION ") {
            let qualified = qualified.split('(').next().unwrap();
            let name = qualified.rsplit('.').next().unwrap();
            assert!(name.len() <= 63, "{name}");
        }
    }
    assert!(!MIGRATION.contains("SKIP LOCKED"));
}

#[test]
fn public_identities_and_result_contracts_are_exact() {
    assert_eq!(
        return_fields(SELECTOR),
        vec![
            "selection_outcome_name TEXT",
            "observed_database_now TIMESTAMPTZ",
            "observed_owner_expires_at TIMESTAMPTZ",
            "selected_drain_intent_id TEXT",
            "selected_source_intent_revision BIGINT",
            "selected_source_state_digest TEXT",
            "selected_source_state_bytes BYTEA",
            "selected_product_operation_id TEXT",
            "selected_product_mutation_digest TEXT",
            "selected_tenant_id TEXT",
            "selected_installation_id TEXT",
            "selected_deployment_id TEXT",
            "selected_expected_revision BIGINT",
            "selected_product_mutation_request_bytes BYTEA",
            "selected_drain_intent_request_bytes BYTEA",
            "selected_drain_intent_digest TEXT",
            "selected_slot_guild_id TEXT",
            "selected_slot_ruleset_key TEXT",
            "selected_target_version BIGINT",
            "selected_target_content_hash TEXT",
            "selected_target_binding_revision BIGINT",
            "selected_target_binding_fingerprint TEXT",
            "predecessor_claim_terminal_digest TEXT",
            "predecessor_gateway_shard_id TEXT",
            "predecessor_process_instance_id TEXT",
            "predecessor_lease_epoch BIGINT",
            "predecessor_runtime_build_revision TEXT",
            "predecessor_owner_revision BIGINT",
            "predecessor_controller_id TEXT",
            "predecessor_controller_fencing_token BIGINT",
            "predecessor_claim_epoch BIGINT",
            "predecessor_claim_revision BIGINT",
            "predecessor_claim_expires_at TIMESTAMPTZ",
            "predecessor_seal_process_instance_id TEXT",
            "predecessor_seal_generation BIGINT",
            "predecessor_seal_observation_sequence BIGINT",
        ]
    );
    assert_eq!(
        return_fields(SUCCESSION),
        vec![
            "journal_outcome_name TEXT",
            "terminal_outcome_name TEXT",
            "recovery_id TEXT",
            "originating_emergency_generation BIGINT",
            "coordinator_generation BIGINT",
            "action_authority_revision BIGINT",
            "selection_authority_revision BIGINT",
            "recovery_class TEXT",
            "observed_gateway_shard_id TEXT",
            "observed_process_instance_id TEXT",
            "observed_lease_epoch BIGINT",
            "observed_runtime_build_revision TEXT",
            "observed_owner_revision BIGINT",
            "database_now TIMESTAMPTZ",
            "observed_owner_expires_at TIMESTAMPTZ",
            "minimum_database_now TIMESTAMPTZ",
            "recorded_at TIMESTAMPTZ",
            "terminal_projection_bytes BYTEA",
            "terminal_digest TEXT",
        ]
    );
    for identity in [SELECTOR_IDENTITY, SUCCESSION_IDENTITY] {
        assert!(MIGRATION.contains(identity), "{identity}");
    }
    assert_eq!(
        MIGRATION
            .lines()
            .filter(|line| line.starts_with("CREATE FUNCTION public."))
            .count(),
        2
    );
}

#[test]
fn capabilities_are_serializable_security_definer_with_read_only_split() {
    let selector = function(SELECTOR);
    let succession = function(SUCCESSION);
    for body in [selector, succession] {
        for required in [
            "LANGUAGE plpgsql",
            "VOLATILE",
            "STRICT",
            "PARALLEL UNSAFE",
            "SECURITY DEFINER",
            "SET search_path = pg_catalog",
            "transaction_isolation",
            "<> 'serializable'",
        ] {
            assert!(body.contains(required), "{required}");
        }
        assert_eq!(body.matches("pg_catalog.clock_timestamp()").count(), 1);
    }
    assert!(!selector.contains("transaction_read_only"));
    assert!(succession.contains("transaction_read_only') <> 'off'"));
    assert!(selector.contains("pg_advisory_xact_lock_shared"));
    assert!(succession.contains("pg_advisory_xact_lock("));
}

#[test]
fn selector_reuses_oldest_order_and_closes_all_source_classes() {
    let selector = function(SELECTOR);
    for required in [
        "starring_runtime_pending_drain_candidate_v2()",
        "selection_outcome_name := 'no_candidate'",
        "selection_outcome_name := 'unclaimed'",
        "THEN 'expired_previous_owner'",
        "ELSE 'fresh_previous_owner'",
        "state_kind = 'pending_unclaimed'",
        "state_kind = 'pending_claimed'",
        "starring_runtime_pending_drain_predecessor_exact_v3(",
        "predecessor_action_row.terminal_digest",
        "selected_source_state_bytes :=",
        "candidate_row.canonical_state_bytes",
        "expected_owner_process_instance_id",
        ")::BIGINT >= expected_owner_lease_epoch",
    ] {
        assert!(selector.contains(required), "{required}");
    }
    assert_eq!(
        selector
            .matches("starring_runtime_pending_drain_candidate_v2()")
            .count(),
        1
    );
}

#[test]
fn replay_fast_path_and_domain_lock_order_are_stable() {
    let succession = function(SUCCESSION);
    let writer = succession.find("starring-runtime-writer-fence-v1").unwrap();
    let owner = succession
        .find("starring-runtime-gateway-owner-v1:")
        .unwrap();
    let action = succession
        .find("starring-runtime-startup-recovery-action-v2:")
        .unwrap();
    let owner_row = succession
        .find("SELECT owner.*\n    INTO owner_row")
        .unwrap();
    let replay = succession
        .find("IF selection_action_found OR authority_action_found THEN")
        .unwrap();
    let slot = succession
        .find("starring-runtime-serving-slot-v1:")
        .unwrap();
    let deployment = succession
        .find("SELECT deployment.*\n    INTO deployment_row")
        .unwrap();
    let product = succession
        .find("SELECT product.*\n    INTO product_row")
        .unwrap();
    let drain = succession
        .find("starring-runtime-drain-intent-v2:")
        .unwrap();
    let predecessor = succession
        .find("WHERE action.recovery_id = predecessor_recovery_id")
        .unwrap();
    let certification = succession
        .find("FROM public.runtime_certification_operations_v2 AS reservation")
        .unwrap();
    assert!(
        writer < owner
            && owner < action
            && action < owner_row
            && owner_row < replay
            && replay < slot
            && slot < deployment
            && deployment < product
            && product < drain
            && drain < certification
            && certification < predecessor
    );
    for required in [
        "existing_action_row.minimum_database_now\n                IS DISTINCT FROM requested_minimum_database_now",
        "requested_minimum_database_now,\n            existing_action_row.terminal_projection_bytes",
        "action_record.outcome_name <> 'replayed'",
        "existing_action_row.terminal_projection_bytes",
        "IS DISTINCT FROM evidence_frame",
    ] {
        assert!(succession.contains(required), "{required}");
    }
}

#[test]
fn predecessor_journal_terminal_and_product_roots_are_revalidated() {
    let succession = function(SUCCESSION);
    let predecessor = function(PREDECESSOR_EXACT);
    for required in [
        "requested_predecessor_claim_terminal_digest",
        "predecessor_action_row.terminal_digest",
        "starring_runtime_pending_drain_predecessor_exact_v3(",
        "starring_runtime_product_mutation_bytes_v2(",
        "starring_runtime_product_mutation_digest_v2(",
        "product_row.product_mutation_request_bytes",
        "candidate_drain_row.drain_intent_request_bytes",
        "runtime_pending_drain_succession_product_root_invalid",
        "'product_mutation_request_digest'",
        "'drain_intent_request_digest'",
    ] {
        assert!(succession.contains(required), "{required}");
    }
    for required in [
        "action_row.record_format_version = 2",
        "action_row.recovery_id = controller_recovery_id",
        "action_row.action_authority_revision =",
        "controller_action_revision",
        "action_row.selection_authority_revision =",
        "controller_action_revision - 1",
        "action_row.coordinator_generation =\n            action_row.originating_emergency_generation + 1",
        "action_row.minimum_database_now <= action_row.recorded_at",
        "action_row.recorded_at < action_row.owner_expires_at",
        "starring_runtime_startup_recovery_terminal_digest_v2(",
        "pg_catalog.convert_from(\n                prior_source_digest_frame,\n                'UTF8'\n            ) ~ '^[0-9a-f]{64}$'",
        "prior_successor_state_frame =",
        "drain_row.canonical_state_bytes",
        "pg_catalog.sha256(prior_successor_state_frame)",
        "drain_row.canonical_state_digest",
        "starring_runtime_pending_drain_product_root_compound_exact_v2(",
        "drain_row.intent_revision - 1",
        "action_row.action_authority_revision",
        "prior_seal_bundle",
        "action_expiry_numeric = claim_expiry_numeric",
    ] {
        assert!(predecessor.contains(required), "{required}");
    }
}

#[test]
fn expiry_boundary_and_direct_successors_are_exact() {
    let selector = function(SELECTOR);
    let succession = function(SUCCESSION);
    let predecessor = function(PREDECESSOR_EXACT);
    let successor = function(SUCCESSOR_EXACT);
    assert!(selector.contains("observed_database_now_numeric >= claim_expiry_numeric"));
    assert!(succession.contains("database_now_numeric < predecessor_claim_expiry_numeric"));
    assert!(selector.contains(
        "predecessor_claim_expires_at :=\n            predecessor_action_row.owner_expires_at"
    ));
    assert!(!MIGRATION.contains("DOUBLE PRECISION"));
    assert!(!MIGRATION.contains("to_timestamp("));
    assert!(predecessor.contains(
        "action_expiry_numeric :=\n        EXTRACT(EPOCH FROM action_row.owner_expires_at) * 1000000"
    ));
    for required in [
        "successor_revision := source_drain_row.intent_revision + 1",
        "successor_claim_revision :=",
        "'{state,claim,claim_revision}')::BIGINT + 1",
        "successor_fencing_token :=\n        deployment_row.last_fencing_token + 1",
        "'recovery:'",
        "requested_action_authority_revision::TEXT",
        "\"state\":{\"kind\":\"route_absent_acknowledged\"",
        "\"acknowledgement\":{\"claim\":",
    ] {
        assert!(succession.contains(required), "{required}");
    }
    for required in [
        "successor_drain.intent_revision =",
        "source_drain.intent_revision + 1",
        "successor_deployment.last_fencing_token =",
        "source_deployment.last_fencing_token + 1",
    ] {
        assert!(successor.contains(required), "{required}");
    }
    assert_eq!(
        succession
            .matches("UPDATE public.runtime_deployments AS deployment")
            .count(),
        1
    );
    assert_eq!(
        succession
            .matches("UPDATE public.runtime_drain_intents_v2 AS drain")
            .count(),
        1
    );
}

#[test]
fn terminal_projection_is_bounded_and_omits_predecessor_state_bytes() {
    let succession = function(SUCCESSION);
    let projection = function(PROJECTION);
    let projection_frame = function(PROJECTION_FRAME);
    for required in [
        "pg_catalog.octet_length(predecessor_frame)\n            NOT BETWEEN 1 AND 8192",
        "pg_catalog.octet_length(successor_state_frame)\n            NOT BETWEEN 1 AND 1048576",
        "pg_catalog.octet_length(recovery_evidence_frame)\n            NOT BETWEEN 1 AND 16384",
        "pg_catalog.octet_length(transition_frame)\n            NOT BETWEEN 1 AND 16384",
        "pg_catalog.octet_length(projection_bytes)\n            NOT BETWEEN 1 AND 131072",
        "pg_catalog.sha256(framed_payload)",
    ] {
        assert!(projection.contains(required), "{required}");
    }
    for required in [
        "projection_length NOT BETWEEN 1 AND 131072",
        "expected_outcome_tag NOT BETWEEN 0 AND 3",
        "requested_frame_index NOT BETWEEN 1 AND 4",
        "pg_catalog.sha256(payload_value)",
    ] {
        assert!(projection_frame.contains(required), "{required}");
    }
    let predecessor_start = succession
        .rfind("predecessor_frame := pg_catalog.convert_to(")
        .unwrap();
    let transition_start = succession[predecessor_start..]
        .find("transition_frame := pg_catalog.convert_to(")
        .map(|offset| predecessor_start + offset)
        .unwrap();
    let predecessor_frame = &succession[predecessor_start..transition_start];
    for required in [
        "'source_state_digest'",
        "'predecessor_claim_terminal_digest'",
        "'predecessor_controller_id'",
        "'predecessor_claim_revision'",
        "'predecessor_claim_source_digest'",
    ] {
        assert!(predecessor_frame.contains(required), "{required}");
    }
    assert!(!predecessor_frame.contains("source_drain_row.canonical_state_bytes"));
    assert!(succession.contains(
        "pg_catalog.strpos(\n                pg_catalog.convert_from(predecessor_frame, 'UTF8'),"
    ));
    assert!(
        succession.contains("source_drain_row.canonical_state_bytes,\n                    'UTF8'")
    );
}

#[test]
fn durable_mutations_precede_one_applied_action_record() {
    let succession = function(SUCCESSION);
    let deployment = succession
        .find("UPDATE public.runtime_deployments AS deployment")
        .unwrap();
    let drain = succession
        .find("UPDATE public.runtime_drain_intents_v2 AS drain")
        .unwrap();
    let applied_record = succession
        .rfind("starring_runtime_startup_recovery_action_record_v2(")
        .unwrap();
    assert!(deployment < drain && drain < applied_record);
    assert_eq!(
        succession
            .matches("starring_runtime_startup_recovery_action_record_v2(")
            .count(),
        2
    );
    assert_eq!(
        succession[drain..]
            .matches("starring_runtime_startup_recovery_action_record_v2(")
            .count(),
        1
    );
    assert!(succession.contains("SET intent_revision = successor_revision"));
    assert!(succession.contains("intent_state = 'route_absent_acknowledged'"));
    assert!(!succession.contains("SET intent_state = 'pending_claimed'"));
    assert!(succession.contains("action_record.outcome_name <> 'applied'"));
}

#[test]
fn manifests_readiness_acl_and_postflight_keep_capabilities_quarantined() {
    let grant = dollar_block("grant_executor");
    let manifest = dollar_block("patch_schema_manifest");
    let readiness = dollar_block("patch_readiness");
    let postflight = dollar_block("postflight");
    for identity in [SELECTOR_IDENTITY, SUCCESSION_IDENTITY] {
        assert!(grant.contains(identity), "{identity}");
        assert!(manifest.contains(identity), "{identity}");
        assert!(readiness.contains(identity), "{identity}");
        assert!(postflight.contains(identity), "{identity}");
        assert!(CONTRACT_SOURCE.contains(identity), "{identity}");
    }
    assert!(CONTRACT_SOURCE.contains("OPERATION_CAPABILITY_IDENTITIES_V1: [&str; 29]"));
    assert!(CONTRACT_SOURCE.contains("capabilities.clone().count() != 31"));
    for identity in [
        PREDECESSOR_IDENTITY,
        SUCCESSOR_IDENTITY,
        PROJECTION_IDENTITY,
        PROJECTION_FRAME_IDENTITY,
    ] {
        assert!(manifest.contains(identity), "{identity}");
        assert!(postflight.contains(identity), "{identity}");
    }
    assert!(manifest.contains("RETURN observed_count = 834"));
    assert!(manifest.contains(MANIFEST_OBSERVED_DIGEST));
    assert!(readiness.contains(MANIFEST_DEFINITION_DIGEST));
    for (identity, digest) in [
        (
            "public.starring_runtime_execution_schema_manifest_v1()",
            MANIFEST_DEFINITION_DIGEST,
        ),
        (
            "public.starring_runtime_execution_database_readiness_v1()",
            READINESS_DEFINITION_DIGEST,
        ),
        (SELECTOR_IDENTITY, SELECTOR_DEFINITION_DIGEST),
        (SUCCESSION_IDENTITY, SUCCESSION_DEFINITION_DIGEST),
        (PREDECESSOR_IDENTITY, PREDECESSOR_DEFINITION_DIGEST),
        (SUCCESSOR_IDENTITY, SUCCESSOR_DEFINITION_DIGEST),
        (PROJECTION_IDENTITY, PROJECTION_DEFINITION_DIGEST),
        (
            PROJECTION_FRAME_IDENTITY,
            PROJECTION_FRAME_DEFINITION_DIGEST,
        ),
    ] {
        let exact = format!("('{identity}', '{digest}')");
        assert!(postflight.contains(&exact), "{exact}");
    }
    for required in [
        "REVOKE ALL ON FUNCTION",
        "invalid_acl_count",
        "invalid_alias_count",
        "invalid_digest_count",
        "manifest_digest",
        "readiness_digest",
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_execution_database_readiness_v1()",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    for forbidden in [
        "GRANT SELECT",
        "GRANT INSERT",
        "GRANT UPDATE",
        "GRANT DELETE",
        "GRANT TRUNCATE",
        "GRANT USAGE ON SCHEMA",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}
