const MIGRATION: &str =
    include_str!("../../../migrations/202607310012_allow_committed_certification_execution.sql");
const PREVIOUS_MIGRATION: &str =
    include_str!("../../../migrations/202607310011_allow_terminalized_stale_live_recovery.sql");
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
fn committed_certification_execution_migration_is_bounded_forward_only_and_comment_free() {
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
fn committed_certification_execution_patches_all_writers_with_exact_terminal_pairs() {
    let patch = dollar_block("patch_execution_writers");
    for required in [
        "public.starring_runtime_execution_claim_next_v1(text,bigint)",
        "public.starring_runtime_execution_renew_v1(text,text,text,bigint,text,bigint,bigint,bigint,bigint)",
        "public.starring_runtime_execution_mutate_v1(text,text,text,bigint,text,bigint,bigint,bigint,text,jsonb)",
        "ARRAY[24, 28]::INTEGER[]",
        "ARRAY[20]::INTEGER[]",
        "ARRAY['deployment', 'deployment_row']::TEXT[]",
        "ARRAY['deployment_row']::TEXT[]",
        "deployment_alias :=",
        "terminal.terminal_outcome_name =",
        "'''awaiting_reset'''",
        "'''reconciling_panels'''",
        "'''certification_committed'''",
        "terminal.resulting_phase = ''live''",
        "AND terminal.resulting_deployment_revision <",
        "AND terminal.resulting_convergence_attempt_no <=",
        "|| deployment_alias",
        "|| '.revision'",
        "|| '.convergence_attempt_no'",
        "metadata_after IS DISTINCT FROM metadata_before",
        "observed_count <> patch_row.expected_occurrences",
        "pg_catalog.strpos(definition, old_predicate) <> 0",
    ] {
        assert!(patch.contains(required), "{required}");
    }
    assert_eq!(patch.matches("ARRAY[20]::INTEGER[]").count(), 2);
    assert_eq!(patch.matches("ARRAY['deployment_row']::TEXT[]").count(), 2);
    assert_eq!(patch.matches("2::BIGINT").count(), 1);
    assert_eq!(patch.matches("1::BIGINT").count(), 2);
    assert_eq!(patch.matches("old_predicate :=").count(), 2);
    assert_eq!(patch.matches("new_predicate :=").count(), 2);
    assert_eq!(patch.matches("'''certification_committed'''").count(), 2);
    assert_eq!(
        patch.matches("terminal.resulting_phase = ''live''").count(),
        2
    );
}

#[test]
fn committed_certification_execution_digest_chain_is_exact_and_current() {
    for required in [
        "50ed71606e880e95720b2628abd765748700ef78bb429bf2be07f739b2aefd1e",
        "2ffaba44876ebfac5b32e0fdd34d147d26be1d83e312534070ca339df244d28e",
        "7c7b1d1884c79c9040eb6937a997ce3d2540eb390fb3f5c757c8d6dfeda16b0e",
        "4478b214a538ef30df57d34cecbc0afba8052bec72b9c989b4a80b31975e44c2",
        "d6972ef0bb0b088480cdfed79da274f183dc3dd61908487d4b8e0339998b2e27",
        "7d436880cd9ba7b95060ce97f6f36c2789c93a537eff4a7197ac5d71a9294c01",
        "053b2cc1576b5a6c6fad441fb5222b60176ad2e4c6581befab383a2d9fb886ee",
        "1d85a38b5d30b20a4b15c6adc70af3e08ea66901465ba83b2d2bf8d200ccbfca",
        "f6bd51c0de1eff13175d07f8861f71e4f08b2e7395cfc3eaf516cf4b644a4e63",
        "2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7",
        "779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e",
        "0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(PREVIOUS_MIGRATION
        .contains("779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e"));
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains("0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f"));
        assert!(
            !source.contains("779d97c088a29027589ebdffa9753eb1333a1d9b511cd714211cde6ae8146c4e")
        );
    }
}

#[test]
fn committed_certification_execution_postflight_preserves_all_runtime_manifests() {
    let postflight = dollar_block("postflight");
    for required in [
        "public.starring_runtime_execution_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v1()",
        "public.starring_runtime_exact_target_schema_manifest_v2()",
        "public.starring_runtime_serving_schema_manifest_v1()",
        "public.starring_runtime_interaction_schema_manifest_v1()",
        "'''certification_committed'''",
        "terminal.resulting_phase = ''live''",
        "2ffaba44876ebfac5b32e0fdd34d147d26be1d83e312534070ca339df244d28e",
        "4478b214a538ef30df57d34cecbc0afba8052bec72b9c989b4a80b31975e44c2",
        "7d436880cd9ba7b95060ce97f6f36c2789c93a537eff4a7197ac5d71a9294c01",
        "2ee6db433ac8976c754c1566b39eb17950d8c9e1a9e5e56d6d96e45a39342dc7",
        "0e69552f26e09949d44b87c7ae7680432ff2c36a0027230efcf541cc4324cd9f",
    ] {
        assert!(postflight.contains(required), "{required}");
    }
}
