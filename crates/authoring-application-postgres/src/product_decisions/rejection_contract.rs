pub(super) const DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_product_rejection_executor_database_identity_v1()";
pub(super) const KEYRING_COVERAGE_FUNCTION: &str =
    "public.starring_product_rejection_keyring_coverage_v1(text[],text[])";
pub(super) const KEYRING_COVERAGE_ARGUMENTS: &str = "idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[]";
pub(super) const KEYRING_COVERAGE_RESULT: &str = "TABLE(outcome text)";
pub(super) const REJECT_FUNCTION: &str = "public.starring_product_reject_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text)";
pub(super) const REJECT_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, expected_rejection_reason text";
pub(super) const REJECT_RESULT: &str = "TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text)";
pub(super) const TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_rejection_executor_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";
