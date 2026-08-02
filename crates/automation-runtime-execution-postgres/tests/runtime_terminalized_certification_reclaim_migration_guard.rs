const MIGRATION: &str =
    include_str!("../../../migrations/202607310009_allow_terminalized_certification_reclaim.sql");
const PREVIOUS_MIGRATION: &str =
    include_str!("../../../migrations/202607310008_refresh_cross_capability_readiness.sql");
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
fn terminalized_reclaim_migration_is_bounded_forward_only_and_comment_free() {
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
fn terminalized_reclaim_is_exact_fail_closed_and_post_lock_revalidated() {
    let patch = dollar_block("patch_claim");
    for required in [
        "old_selector",
        "new_selector",
        "old_locked",
        "new_locked",
        "terminal.operation_id = reservation.operation_id",
        "terminal.intent_fingerprint = reservation.intent_fingerprint",
        "terminal.tenant_id = reservation.tenant_id",
        "terminal.installation_id = reservation.installation_id",
        "terminal.deployment_id = reservation.deployment_id",
        "terminal.deployment_revision = reservation.deployment_revision",
        "terminal.convergence_attempt_no = reservation.convergence_attempt_no",
        "terminal.terminal_outcome_name = 'awaiting_reset'",
        "terminal.resulting_phase = 'reconciling_panels'",
        "deployment.phase = 'reconciling_panels'",
        "deployment_row.phase = 'reconciling_panels'",
        "terminal.resulting_deployment_revision",
        "terminal.resulting_convergence_attempt_no",
        "AND NOT EXISTS (",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    assert_eq!(
        patch
            .matches("terminal.terminal_outcome_name = 'awaiting_reset'")
            .count(),
        2
    );
    assert!(!patch.contains("certification_committed"));
    assert!(patch.contains("50ed71606e880e95720b2628abd765748700ef78bb429bf2be07f739b2aefd1e"));
}

#[test]
fn terminalized_reclaim_refreshes_execution_manifest_and_all_current_pins() {
    for required in [
        "cc5475b256b6b48f3c4f6d3933461cdcdeff1dbdb974d32d7d735348d8f14eb4",
        "50ed71606e880e95720b2628abd765748700ef78bb429bf2be07f739b2aefd1e",
        "dd7a64d16d27a32dde6f80416e4efc444c69aa59e055ff26f8008a2cdc845a62",
        "7597ea370c26ac6b5534e1568637e79faad000184e6a862f59acba56276a6a40",
        "ee35572e966037477a9070fef87781e901f0ef49e3cb471acebba9c165657676",
        "34cde5bd3a13f2132ba29f5324e67c95cf7511ea04aaa1033026289d70267027",
        "437eef0962f31be61e9fcb2f6705b2cda14f4d52105ae024ca4bc29b967e001c",
        "d8e46c1204b36b3c909b7e6e88ee768d2ec7e60d05dd4eb99be7d8f064a24714",
        "metadata_after IS DISTINCT FROM metadata_before",
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "public.starring_runtime_serving_schema_manifest_v1()",
        "public.starring_runtime_interaction_schema_manifest_v1()",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(PREVIOUS_MIGRATION
        .contains("16ac5e4726c5ab72da45c1ab67490a50e737197d79a435133fcbd27b56f79a15"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains("98ed1251e3339ffb452ed12334699e93f43e2ea3cd7d327bc3d2a11fe12b9fb2"));
    }
}
