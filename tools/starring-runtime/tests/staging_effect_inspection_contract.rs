const INSPECTION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ops/postgres/staging-runtime-interaction-effect-inspection.sql"
));
const EFFECT_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/automation-runtime-interaction-postgres/src/effect.rs"
));
const RUNBOOK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/superpowers/runbooks/2026-07-29-macos-starring-runtime-staging-operations.md"
));

const RECOVERY_BLOCK_CODES: [&str; 10] = [
    "recovery_blocked_discord_read_rejected",
    "recovery_blocked_response_token_unavailable",
    "recovery_blocked_observation_protocol",
    "recovery_blocked_compensation_conflict",
    "recovery_blocked_compensation_unsupported",
    "recovery_blocked_non_compensable",
    "recovery_blocked_internal_conflict",
    "recovery_blocked_discord_forbidden",
    "recovery_blocked_internal_authority",
    "recovery_blocked_attempt_budget_exhausted",
];

fn section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).unwrap();
    let remaining = &source[start..];
    let end = remaining.find(end).unwrap();
    &remaining[..end]
}

#[test]
fn effect_inspection_is_read_only_target_bound_and_fail_closed() {
    for required in [
        "\\set ON_ERROR_STOP on",
        ":{?expected_database}",
        ":{?expected_system_identifier}",
        ":{?runtime_dedicated_cluster_acknowledgement}",
        "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY;",
        "SET LOCAL search_path = pg_catalog;",
        "BETWEEN 160000 AND 169999",
        "pg_catalog.current_database() = 'starring_runtime_staging'",
        "current_user = session_user",
        "current_user = 'starring_cluster_admin'",
        "role.rolsuper",
        "role.rolcanlogin",
        "pg_catalog.pg_control_system()",
        "starring-runtime-dedicated-staging-cluster-v2:%s:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation",
        "ledger.observed_count = 122",
        "fbfb4a3fec87d3e142c467bbcefa5ccc573b2ff8ae66d71c0f49d20240dbb294",
        "migration.version = 202608040001",
        "0176a67c84119b64791f2c3190c371802eff2726c0005a1d0636176f672be9df837d4436dcf26f49c32297f59b0fb3b0",
        "public.starring_runtime_interaction_effect_schema_manifest_v1()",
        "event.event_revision = head.head_revision",
        "terminal.event_kind IS DISTINCT FROM 'recovery_required'",
        "terminal.to_state IS DISTINCT FROM 'recovery_required'",
        "terminal.outcome_code NOT IN (",
        "\\if :effect_inspection_projection_valid",
        "\\quit 3",
        "COMMIT;",
    ] {
        assert!(INSPECTION.contains(required), "missing contract: {required}");
    }

    assert_eq!(INSPECTION.matches("BEGIN TRANSACTION").count(), 1);
    assert_eq!(INSPECTION.matches("COMMIT;").count(), 1);
    assert!(INSPECTION.matches("\\quit 3").count() >= 6);
    for forbidden in [
        "\nINSERT ",
        "\nUPDATE ",
        "\nDELETE ",
        "\nTRUNCATE ",
        "\nCREATE ",
        "\nALTER ",
        "\nDROP ",
        "\nGRANT ",
        "\nREVOKE ",
        "\nDO $",
        "LOCK TABLE",
        "pg_advisory",
        "SECURITY DEFINER",
    ] {
        assert!(
            !INSPECTION.contains(forbidden),
            "forbidden SQL: {forbidden}"
        );
    }
}

#[test]
fn effect_inspection_exports_only_the_exact_redacted_aggregate() {
    let validation = section(
        INSPECTION,
        "WITH blocked_heads AS (",
        "\\gset\n\n\\if :effect_inspection_projection_valid",
    );
    let code_impl = section(
        EFFECT_SOURCE,
        "impl RuntimeInteractionEffectRecoveryBlockReasonV1 {",
        "    fn allows_observation",
    );
    for code in RECOVERY_BLOCK_CODES {
        assert_eq!(validation.matches(code).count(), 1, "{code}");
        assert_eq!(code_impl.matches(code).count(), 1, "{code}");
    }

    let output = section(
        INSPECTION,
        "SELECT terminal.outcome_code AS recovery_block_code,",
        "FROM terminal_events AS terminal",
    );
    for required in [
        "terminal.outcome_code AS recovery_block_code",
        "terminal.action_kind",
        "pg_catalog.count(*) AS blocked_effect_count",
        "pg_catalog.min(terminal.observed_at) AS oldest_blocked_at",
        "pg_catalog.max(terminal.observed_at) AS newest_blocked_at",
    ] {
        assert!(output.contains(required), "missing output: {required}");
    }
    for forbidden in [
        "application_id",
        "interaction_id",
        "action_index",
        "output_id",
        "digest",
        "correlation",
        "marker",
        "token",
        "input",
        "preimage",
        "payload",
        "resolved",
        "SELECT *",
    ] {
        assert!(!output.contains(forbidden), "exposed output: {forbidden}");
    }
}

#[test]
fn runtime_runbook_executes_keychain_backed_inspection_and_maps_every_code() {
    let runbook = section(
        RUNBOOK,
        "## Inspect durable interaction effect recovery blocks",
        "## Bootstrap the five database roles",
    );
    for required in [
        "ops/postgres/staging-runtime-interaction-effect-inspection.sql",
        "set +x",
        "umask 077",
        "security find-generic-password -w",
        "-s starring.postgres.staging",
        "-a database.cluster-admin",
        "ADMIN_PGPASS_PATH=\"$ADMIN_PGPASS_DIR/pgpass\"",
        "PGPASSFILE=\"$ADMIN_PGPASS_PATH\" PGSSLMODE=disable",
        "--no-psqlrc --set ON_ERROR_STOP=1 --no-password",
        "--username \"$STARRING_STAGING_CLUSTER_ADMIN\"",
        "--set expected_system_identifier=\"$STARRING_STAGING_EXPECTED_SYSTEM_IDENTIFIER\"",
        "--set runtime_dedicated_cluster_acknowledgement=\"$STARRING_STAGING_DEDICATED_CLUSTER_ACKNOWLEDGEMENT\"",
        "never emits application, interaction,",
        "action, or Discord output identifiers",
        "An unknown code",
        "Do not replay an interaction, delete an effect row, or",
        "edit a journal row",
    ] {
        assert!(runbook.contains(required), "missing runbook contract: {required}");
    }
    assert!(runbook.contains("unset PGPASSWORD"));
    assert!(!runbook.contains("PGPASSWORD="));
    for code in RECOVERY_BLOCK_CODES {
        assert_eq!(runbook.matches(code).count(), 1, "{code}");
    }
}
