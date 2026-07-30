const MIGRATION: &str = include_str!(
    "../../../migrations/202607290001_persist_runtime_ingress_open_acknowledgement_v2.sql"
);
const CONTRACT_SOURCE: &str = include_str!("../src/contract.rs");
const DATABASE_SOURCE: &str = include_str!("../src/database.rs");
const SECURITY_SUPPORT_SOURCE: &str = include_str!("postgres_security/support.rs");

const OBSERVE_IDENTITY: &str =
    "public.starring_runtime_ingress_open_acknowledgement_observe_v2(text)";
const PUBLISH_IDENTITY: &str =
    "public.starring_runtime_ingress_open_acknowledgement_publish_v2(text,bigint,bytea,bytea,bigint,bigint,text,bigint,text,bigint,timestamp with time zone,timestamp with time zone,bigint,bigint,bigint,bigint,bigint)";
const MANIFEST_DEFINITION_DIGEST: &str =
    "72ab1200d416d069371db605ffef6f5f6197fc3f9c0fdd241001d43dd9c82434";
const MIGRATION_READINESS_DEFINITION_DIGEST: &str =
    "572d7ffd19d6f2edb5ec84ea6b7bfebd178c7da0568bce61af2f7907cfe72647";
const LATEST_READINESS_DEFINITION_DIGEST: &str =
    "7bd23bbaa7cef9cfcb88ac6a273dc6ac82af3e55e5ab71fff5a54b98cd90f81e";

fn dollar_block(tag: &str) -> &'static str {
    MIGRATION
        .split(&format!("DO ${tag}$"))
        .nth(1)
        .unwrap()
        .split(&format!("${tag}$;"))
        .next()
        .unwrap()
}

fn publisher() -> &'static str {
    MIGRATION
        .split("CREATE FUNCTION public.starring_runtime_ingress_open_acknowledgement_publish_v2(")
        .nth(1)
        .unwrap()
        .split("$function$;")
        .next()
        .unwrap()
}

#[test]
fn migration_is_atomic_collision_safe_and_comment_free() {
    let preflight = dollar_block("preflight");
    let table = MIGRATION
        .find("CREATE TABLE public.runtime_ingress_open_acknowledgements_v2")
        .unwrap();
    let manifest = MIGRATION.find("DO $patch_schema_manifest$").unwrap();
    let readiness = MIGRATION.find("DO $patch_readiness$").unwrap();
    let postflight = MIGRATION.find("DO $postflight$").unwrap();
    assert!(MIGRATION.starts_with("SET LOCAL lock_timeout = '5s';"));
    assert!(MIGRATION.contains("IN ACCESS EXCLUSIVE MODE;"));
    assert!(table < manifest && manifest < readiness && readiness < postflight);
    for required in [
        "collision_count",
        "manifest_digest",
        "readiness_digest",
        "b7ee8d2a13ae38a88bc1b2558b018e74893e7d90ccd72d96187197a111432e22",
        "3fe2924d130e93d630960be796e3986884fefedddfb91c0dd5b680a41b440cb1",
    ] {
        assert!(preflight.contains(required), "{required}");
    }
    for line in MIGRATION.lines() {
        let trimmed = line.trim_start();
        assert!(!trimmed.starts_with("--"));
        assert!(!trimmed.starts_with("//"));
        assert!(!trimmed.starts_with("/*"));
    }
}

#[test]
fn singleton_row_is_digest_bound_revisioned_and_fail_closed() {
    for required in [
        "gateway_shard_id TEXT PRIMARY KEY",
        "gateway_shard_id = 'shard:0'",
        "source_acknowledgement_revision",
        "request_digest BYTEA NOT NULL",
        "canonical_request_bytes BYTEA NOT NULL",
        "request_digest =",
        "pg_catalog.sha256(canonical_request_bytes)",
        "THEN 197",
        "THEN 578",
        "ELSE 205",
        "ELSE 586",
        "pg_catalog.isfinite(requested_owner_observed_at)",
        "pg_catalog.isfinite(requested_owner_expires_at)",
        "pg_catalog.isfinite(acknowledged_at)",
        "pg_catalog.isfinite(expires_at)",
        "EXTRACT(",
        "EPOCH FROM requested_owner_observed_at",
        "-62135596800000000 AND 253402300799999999",
        "acknowledgement_revision =",
        "COALESCE(source_acknowledgement_revision + 1, 1)",
        "resume_sequence > connected_event_sequence",
        "expires_at <= requested_owner_expires_at",
        "runtime_ingress_open_acknowledgements_v2_validate_transition",
        "runtime_ingress_open_acknowledgements_v2_reject_delete",
        "runtime_ingress_open_acknowledgements_v2_reject_truncate",
    ] {
        assert!(MIGRATION.contains(required), "{required}");
    }
    assert!(!MIGRATION.contains("GRANT SELECT ON TABLE"));
    assert!(!MIGRATION.contains("GRANT INSERT ON TABLE"));
    assert!(!MIGRATION.contains("GRANT UPDATE ON TABLE"));
    assert!(!MIGRATION.contains("GRANT DELETE ON TABLE"));
}

#[test]
fn publisher_uses_one_database_clock_after_canonical_lock_order() {
    let body = publisher();
    let writer = body.find("'starring-runtime-writer-fence-v1'").unwrap();
    let owner = body.find("'starring-runtime-gateway-owner-v1:'").unwrap();
    let acknowledgement = body
        .find("'starring-runtime-ingress-open-acknowledgement-v2:'")
        .unwrap();
    let clock = body
        .find("database_now := pg_catalog.clock_timestamp();")
        .unwrap();
    assert!(writer < owner && owner < acknowledgement && acknowledgement < clock);
    for required in [
        "FOR UPDATE",
        "writer_fence_row.fence_state <> 'open'",
        "owner_row.expires_at <= database_now",
        "source_acknowledgement_revision",
        "runtime_ingress_open_acknowledgement_cas_lost",
        "LEAST(",
        "requested_lease_milliseconds",
        "owner_row.expires_at",
        "WHEN database_now >= acknowledgement_row.expires_at",
        "THEN 'not_current'::TEXT",
    ] {
        assert!(body.contains(required), "{required}");
    }
    let conflict = body
        .find(
            "IF acknowledgement_row.request_digest\n                IS DISTINCT FROM proposed_request_digest",
        )
        .unwrap();
    let conflict_return = body[conflict..]
        .find("'not_current'::TEXT")
        .map(|offset| conflict + offset)
        .unwrap();
    let replay_corrupt = body
        .find("runtime_ingress_open_acknowledgement_replay_corrupt")
        .unwrap();
    assert!(conflict < conflict_return && conflict_return < replay_corrupt);
    assert!(!body.contains("SKIP LOCKED"));
    assert!(!body.contains("pg_catalog.least"));
}

#[test]
fn executor_surface_is_function_only_and_exclusive() {
    for identity in [OBSERVE_IDENTITY, PUBLISH_IDENTITY] {
        assert!(MIGRATION.contains(identity), "{identity}");
        assert!(CONTRACT_SOURCE.contains(identity), "{identity}");
        assert!(SECURITY_SUPPORT_SOURCE.contains(identity), "{identity}");
    }
    assert!(MIGRATION.contains("REVOKE ALL ON FUNCTION"));
    assert!(MIGRATION.contains("runtime_ingress_open_acknowledgement_execution_acl_drift"));
    assert!(MIGRATION.contains("grantee_count > 1"));
    assert!(MIGRATION.contains("privilege.grantee IS DISTINCT FROM"));
    assert_eq!(
        MIGRATION
            .matches("GRANT EXECUTE ON FUNCTION %s TO %I")
            .count(),
        1
    );
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

#[test]
fn manifest_readiness_and_rust_pins_advance_together() {
    assert!(MIGRATION.contains("RETURN observed_count = 948"));
    assert!(MIGRATION.contains("bd8e47e52db30d06ac726b2763a20f54b993f1e04c374975a96a510a31919ade"));
    for digest in [
        MANIFEST_DEFINITION_DIGEST,
        MIGRATION_READINESS_DEFINITION_DIGEST,
    ] {
        assert!(MIGRATION.contains(digest), "{digest}");
    }
    for source in [CONTRACT_SOURCE, DATABASE_SOURCE, SECURITY_SUPPORT_SOURCE] {
        assert!(source.contains(LATEST_READINESS_DEFINITION_DIGEST));
    }
    assert!(CONTRACT_SOURCE.contains("OPERATION_CAPABILITY_IDENTITIES_V1: [&str; 29]"));
    assert!(CONTRACT_SOURCE.contains("capabilities.clone().count() != 31"));
    assert!(SECURITY_SUPPORT_SOURCE.contains("const EXECUTOR_FUNCTIONS: [&str; 31]"));
    let postflight = dollar_block("postflight");
    assert!(postflight.contains("NOT public.starring_runtime_execution_schema_manifest_v1()"));
    assert!(postflight.contains("invalid_acl_count"));
    assert!(postflight.contains("invalid_executor_count"));
}
