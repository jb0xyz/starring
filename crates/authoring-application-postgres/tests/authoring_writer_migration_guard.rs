const MIGRATION: &str =
    include_str!("../../../migrations/202607300001_add_trusted_authoring_generation_writer.sql");

#[test]
fn trusted_writer_schema_is_legacy_compatible_and_projection_exact() {
    for column in [
        "writer_semantic_request_digest TEXT",
        "writer_digest_key_id TEXT",
        "writer_digest_key_fingerprint TEXT",
        "safe_turn_projection BYTEA",
        "safe_turn_projection_digest TEXT",
    ] {
        assert!(MIGRATION.contains(column), "missing {column}");
    }
    for required in [
        "authoring_generations_writer_metadata_presence_valid",
        "writer_semantic_request_digest IS NULL",
        "writer_semantic_request_digest IS NOT NULL",
        "pg_catalog.octet_length(safe_turn_projection)",
        "stage IN (",
        "'needs_input'",
        "'discussion'",
        "'capability_gap'",
        "'preview_ready'",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(!MIGRATION.contains("safe_turn_projection JSONB"));
}

#[test]
fn writer_surface_is_exact_scoped_and_relation_blind() {
    for identity in [
        "starring_authoring_session_writer_database_identity_v1()",
        "starring_authoring_session_writer_check_v1(",
        "starring_authoring_session_writer_load_v1(",
        "starring_authoring_session_writer_commit_v1(",
        "starring_authoring_session_writer_key_coverage_v1(",
        "starring_product_authorized_snapshot_read_v2(",
    ] {
        assert!(MIGRATION.contains(identity), "missing {identity}");
    }
    assert_eq!(MIGRATION.matches("SECURITY DEFINER").count(), 6);
    assert_eq!(MIGRATION.matches("SET search_path = pg_catalog").count(), 6);
    assert!(MIGRATION.contains("REVOKE ALL PRIVILEGES ON FUNCTION"));
    assert!(MIGRATION.contains("FROM PUBLIC CASCADE"));
    assert!(!MIGRATION.contains("GRANT SELECT ON"));
    assert!(!MIGRATION.contains("GRANT INSERT ON"));
    assert!(!MIGRATION.contains("GRANT UPDATE ON"));
    assert!(!MIGRATION.contains("GRANT DELETE ON"));
}

#[test]
fn atomic_commit_prioritizes_replay_and_rechecks_authority() {
    for required in [
        "pg_catalog.pg_advisory_xact_lock",
        "FOR UPDATE",
        "outcome_code := 'exact_replay'",
        "outcome_code := 'idempotency_conflict'",
        "outcome_code := 'generation_conflict'",
        "outcome_code := 'authority_conflict'",
        "outcome_code := 'binding_conflict'",
        "outcome_code := 'committed'",
        "INSERT INTO public.authoring_sessions",
        "INSERT INTO public.authoring_session_generations",
        "UPDATE public.authoring_sessions",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    let replay = MIGRATION
        .find("outcome_code := 'exact_replay'")
        .expect("exact replay outcome");
    let generation_conflict = MIGRATION
        .find("outcome_code := 'generation_conflict'")
        .expect("generation conflict outcome");
    assert!(replay < generation_conflict);
    for forbidden in [
        "raw_model_response",
        "backend_error",
        "human_message TEXT",
        "idempotency_key TEXT",
    ] {
        assert!(!MIGRATION.contains(forbidden), "forbidden {forbidden}");
    }
}
