const MIGRATION: &str =
    include_str!("../../../migrations/202607310011_allow_terminalized_stale_live_recovery.sql");
const PREVIOUS_MIGRATION: &str = include_str!(
    "../../../migrations/202607310010_allow_terminalized_certification_execution_writers.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

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
fn terminalized_stale_live_recovery_migration_is_bounded_forward_only_and_comment_free() {
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.ends_with("RESET lock_timeout;\n"));
    assert!(MIGRATION.contains("starring-runtime-writer-fence-v1"));
    assert!(MIGRATION.contains("IN ACCESS SHARE MODE;"));
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
    for forbidden in [
        "GRANT ",
        "REVOKE ",
        "DROP ",
        "DELETE FROM",
        "UPDATE public.",
        "INSERT INTO public.",
    ] {
        assert!(!MIGRATION.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn stale_live_classifier_partitions_unresolved_and_exact_terminal_roots() {
    let patch = dollar_block("patch_stale_live");
    for required in [
        "terminal.operation_id IS NULL",
        "deployment.phase = 'awaiting_gateway_ready'",
        "terminal.operation_id IS NOT NULL",
        "terminal.operation_id = reservation.operation_id",
        "terminal.intent_fingerprint =",
        "reservation.intent_fingerprint",
        "terminal.tenant_id = reservation.tenant_id",
        "terminal.installation_id =",
        "reservation.installation_id",
        "terminal.deployment_id =",
        "terminal.deployment_revision =",
        "reservation.deployment_revision",
        "terminal.convergence_attempt_no =",
        "reservation.convergence_attempt_no",
        "terminal.terminal_outcome_name =",
        "'awaiting_reset'",
        "terminal.resulting_phase =",
        "'reconciling_panels'",
        "'certification_committed'",
        "terminal.resulting_phase = 'live'",
        "terminal.resulting_deployment_revision =",
        "reservation.deployment_revision + 1",
        "terminal.resulting_convergence_attempt_no =",
        "reservation.convergence_attempt_no",
        "deployment.snapshot ->> 'revision' =",
        "deployment.revision::TEXT",
        "deployment.snapshot #>> '{phase,phase}' =",
        "deployment.phase",
        "deployment.revision =",
        "terminal.resulting_deployment_revision",
        "deployment.phase =",
        "terminal.resulting_phase",
        "deployment.convergence_attempt_no =",
        "terminal.resulting_convergence_attempt_no",
        "deployment.revision >",
        "deployment.convergence_attempt_no >=",
        "invalid_reservation_count := reservation_count",
        "- unresolved_reservation_count",
        "- exact_terminal_reservation_count",
        "IF invalid_reservation_count <> 0 THEN",
        "IF unresolved_reservation_count > 4294967295",
        "metadata_after IS DISTINCT FROM metadata_before",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    assert!(patch.contains("/ pg_catalog.char_length(old_declaration) <> 1"));
    assert!(patch.contains("/ pg_catalog.char_length(old_classifier) <> 1"));
    assert!(patch.contains("/ pg_catalog.char_length(old_bound) <> 1"));
    assert!(patch.contains("/ pg_catalog.char_length(new_declaration) <> 1"));
    assert!(patch.contains("/ pg_catalog.char_length(new_classifier) <> 1"));
    assert!(patch.contains("/ pg_catalog.char_length(new_bound) <> 1"));
    assert!(!patch.contains("SKIP LOCKED"));
}

#[test]
fn terminalized_stale_live_recovery_digest_chain_is_exact_and_current() {
    for required in [
        "11aeeae9eb23564951a87c947439c0a6f87c5dca1b506a1cb9b5e0f4f9c0c936",
        "31ec76b9dbbde23f3caa66e2435ddac8a64755729e14385a3baf96dd8060c8fd",
        "0235fe476513635ca25c6ec752c26386e1d7d4d317212e2c3ecbeb1f6306f766",
        "053b2cc1576b5a6c6fad441fb5222b60176ad2e4c6581befab383a2d9fb886ee",
        "2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03",
        "f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63",
        "7bd23bbaa7cef9cfcb88ac6a273dc6ac82af3e55e5ab71fff5a54b98cd90f81e",
        "779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(PREVIOUS_MIGRATION
        .contains("7bd23bbaa7cef9cfcb88ac6a273dc6ac82af3e55e5ab71fff5a54b98cd90f81e"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains("0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f"));
    }
}

#[test]
fn terminalized_stale_live_recovery_preserves_all_runtime_manifests() {
    let postflight = dollar_block("postflight");
    for required in [
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "public.starring_runtime_serving_schema_manifest_v1()",
        "public.starring_runtime_interaction_schema_manifest_v1()",
        "LEFT JOIN public.runtime_certification_operation_terminals_v2",
        "terminal.operation_id = reservation.operation_id",
        "invalid_reservation_count := reservation_count",
        "IF unresolved_reservation_count > 4294967295",
        "31ec76b9dbbde23f3caa66e2435ddac8a64755729e14385a3baf96dd8060c8fd",
        "f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63",
        "779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}
