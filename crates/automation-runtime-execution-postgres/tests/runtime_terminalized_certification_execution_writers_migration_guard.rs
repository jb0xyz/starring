const MIGRATION: &str = include_str!(
    "../../../migrations/202607310010_allow_terminalized_certification_execution_writers.sql"
);
const PREVIOUS_MIGRATION: &str =
    include_str!("../../../migrations/202607310009_allow_terminalized_certification_reclaim.sql");
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
fn terminalized_execution_writer_migration_is_bounded_forward_only_and_comment_free() {
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
fn terminalized_execution_writers_preserve_exact_fail_closed_terminal_identity() {
    let patch = dollar_block("patch_execution_writers");
    for required in [
        "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
        "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
        "terminal.operation_id = reservation.operation_id",
        "terminal.intent_fingerprint = reservation.intent_fingerprint",
        "terminal.tenant_id = reservation.tenant_id",
        "terminal.installation_id = reservation.installation_id",
        "terminal.deployment_id = reservation.deployment_id",
        "terminal.deployment_revision = reservation.deployment_revision",
        "terminal.convergence_attempt_no = reservation.convergence_attempt_no",
        "terminal.terminal_outcome_name = 'awaiting_reset'",
        "terminal.resulting_phase = 'reconciling_panels'",
        "terminal.resulting_deployment_revision",
        "terminal.resulting_convergence_attempt_no",
        "deployment_row.phase = 'reconciling_panels'",
        "AND NOT EXISTS (",
        "metadata_after IS DISTINCT FROM metadata_before",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    assert_eq!(
        patch
            .matches("terminal.terminal_outcome_name = 'awaiting_reset'")
            .count(),
        1
    );
    assert!(!patch.contains("certification_committed"));
    assert!(patch.contains("old_guard"));
    assert!(patch.contains("new_guard"));
    assert!(patch.contains("/ pg_catalog.char_length(old_guard) <> 1"));
    assert!(patch.contains("/ pg_catalog.char_length(new_guard) <> 1"));
}

#[test]
fn terminalized_execution_writer_digest_chain_is_exact_and_current() {
    for required in [
        "00fb1426fd8711b496b35e0658db13a534560ba13191d710c4274cd54461275c",
        "7c7b1d1884c79c9040eb6937a997ce3d2540eb390fb3f5c757c8d6dfeda16b0e",
        "9e201e149dac432794bfcfc23b424f59741869fcf9d39765693a21b2451646ce",
        "d6972ef0bb0b088480cdfed79da274f183dc3dd61908487d4b8e0339998b2e27",
        "7597ea370c26ac6b5534e1568637e79faad000184e6a862f59acba56276a6a40",
        "0235fe476513635ca25c6ec752c26386e1d7d4d317212e2c3ecbeb1f6306f766",
        "34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027",
        "2d64e05eaf87f593c181fef92a4131539940fd4e58ac5acfbd33a4c39f8d2f03",
        "d8e46c1204b36b3c909b7e6e88ee768d2ec7e60d05dd4eb99be7d8f064a24714",
        "7bd23bbaa7cef9cfcb88ac6a273dc6ac82af3e55e5ab71fff5a54b98cd90f81e",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(PREVIOUS_MIGRATION
        .contains("d8e46c1204b36b3c909b7e6e88ee768d2ec7e60d05dd4eb99be7d8f064a24714"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains("98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2"));
        assert!(
            !source.contains("d8e46c1204b36b3c909b7e6e88ee768d2ec7e60d05dd4eb99be7d8f064a24714")
        );
    }
}

#[test]
fn terminalized_execution_writer_postflight_preserves_lock_order_and_manifests() {
    let postflight = dollar_block("postflight");
    for required in [
        "writer_position < slot_position",
        "slot_position < physical_position",
        "physical_position < deployment_lock_position",
        "deployment_lock_position < reservation_position",
        "reservation_position < continuation_position",
        "guard_count <> 1",
        "terminal_count <> 1",
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "public.starring_runtime_serving_schema_manifest_v1()",
        "public.starring_runtime_interaction_schema_manifest_v1()",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}
