pub(super) const DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_product_apply_executor_database_identity_v1()";
pub(super) const LOCK_FUNCTION: &str = "public.starring_product_apply_lock_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text)";
pub(super) const LOCK_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text";
pub(super) const LOCK_RESULT: &str = "TABLE(outcome text, exact_replay boolean, requires_commit boolean, resulting_revision bigint, resulting_state text, deployment_id text, desired_target_digest text, locked_projection jsonb)";
pub(super) const BEGIN_RUNTIME_DRAIN_FUNCTION: &str = "public.starring_product_apply_begin_runtime_drain_v2(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,text,text)";
pub(super) const BEGIN_RUNTIME_DRAIN_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, proposed_product_operation_id text, proposed_drain_intent_id text";
pub(super) const BEGIN_RUNTIME_DRAIN_RESULT: &str = "TABLE(outcome text, locked_snapshot jsonb, observed_at timestamp with time zone, product_tenant_id text, product_installation_id text, product_deployment_id text, product_expected_revision bigint, product_operation_id text, product_expected_target jsonb, product_mutation_request_bytes bytea, product_mutation_digest text, drain_tenant_id text, drain_installation_id text, drain_deployment_id text, drain_slot_guild_id text, drain_slot_ruleset_key text, drain_expected_revision bigint, drain_intent_id text, drain_intent_request_bytes bytea, drain_intent_digest text, intent_revision bigint, intent_state text, canonical_state_bytes bytea, canonical_state_digest text, writer_epoch_before bigint, writer_epoch_after bigint, pending_drain_intent_id text, pending_product_operation_id text, pending_tenant_id text, pending_installation_id text, pending_deployment_id text, pending_expected_revision bigint, pending_marked_at timestamp with time zone)";
pub(super) const TARGET_ARTIFACT_FUNCTION: &str =
    "public.starring_product_apply_target_artifact_v1(text,text,text,text,bytea,text,text)";
pub(super) const TARGET_ARTIFACT_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_discord_user_id text, expected_guild_id text";
pub(super) const TARGET_ARTIFACT_RESULT: &str = "TABLE(schema_version bigint, definition jsonb, content_hash text, canonical_content_hash text)";
pub(super) const FINALIZE_FUNCTION: &str = "public.starring_product_apply_finalize_v1(text,text,text,bigint,text,text,bytea,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text[],text[],text[],text,text,text,text,text,text,jsonb,text,jsonb,jsonb,jsonb)";
pub(super) const FINALIZE_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_promotion_id text, expected_product_revision bigint, expected_payload_digest text, expected_principal_id text, expected_product_session_digest bytea, session_subject_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, expected_authority_revision bigint, expected_authority_payload_digest text, expected_authority_observation_digest text, expected_authority_observed_at timestamp with time zone, expected_authority_expires_at timestamp with time zone, expected_effective_permission_bits text, expected_guild_owner boolean, product_request_id text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text, new_apply_attempt_id text, new_deployment_id text, locked_projection jsonb, prepared_desired_target_digest text, prepared_previous_runtime jsonb, prepared_snapshot jsonb, prepared_activation_notices jsonb";
pub(super) const FINALIZE_RESULT: &str = "TABLE(outcome text, resulting_revision bigint, resulting_state text, exact_replay boolean, guild_id text, deployment_id text, desired_target_digest text)";
pub(super) const KEYRING_COVERAGE_FUNCTION: &str =
    "public.starring_product_apply_keyring_coverage_v1(text[],text[])";
pub(super) const KEYRING_COVERAGE_ARGUMENTS: &str = "idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[]";
pub(super) const KEYRING_COVERAGE_RESULT: &str = "TABLE(outcome text)";
pub(super) const TOPOLOGY_QUERY: &str = "SELECT \
     public.starring_product_apply_executor_database_identity_v1(), \
     current_database()::TEXT, current_user::TEXT, session_user::TEXT";
pub(super) const LOCK_QUERY: &str = "SELECT outcome, exact_replay, requires_commit, \
    resulting_revision, resulting_state, deployment_id, desired_target_digest, \
    locked_projection FROM public.starring_product_apply_lock_v1(\
        expected_tenant_id => $1, expected_installation_id => $2, \
        expected_promotion_id => $3, expected_product_revision => $4, \
        expected_payload_digest => $5, expected_principal_id => $6, \
        expected_product_session_digest => $7, session_subject_digest => $8, \
        expected_acting_user_id => $9, expected_discord_application_id => $10, \
        expected_guild_id => $11, expected_capability => $12, \
        expected_authority_revision => $13, expected_authority_payload_digest => $14, \
        expected_authority_observation_digest => $15, expected_authority_observed_at => $16, \
        expected_authority_expires_at => $17, expected_effective_permission_bits => $18, \
        expected_guild_owner => $19, product_request_id => $20, \
        active_idempotency_key_digest => $21, idempotency_key_digest_candidates => $22, \
        idempotency_digest_key_id_candidates => $23, \
        idempotency_digest_key_fingerprint_candidates => $24, \
        idempotency_digest_key_id => $25, semantic_request_digest => $26, \
        new_receipt_id => $27, new_audit_event_id => $28, \
        new_apply_attempt_id => $29, new_deployment_id => $30)";
pub(super) const BEGIN_RUNTIME_DRAIN_QUERY: &str = "SELECT outcome, locked_snapshot, observed_at, \
    product_tenant_id, product_installation_id, product_deployment_id, \
    product_expected_revision, product_operation_id, product_expected_target, \
    product_mutation_request_bytes, product_mutation_digest, drain_tenant_id, \
    drain_installation_id, drain_deployment_id, drain_slot_guild_id, \
    drain_slot_ruleset_key, drain_expected_revision, drain_intent_id, \
    drain_intent_request_bytes, drain_intent_digest, intent_revision, intent_state, \
    canonical_state_bytes, canonical_state_digest, writer_epoch_before, writer_epoch_after, \
    pending_drain_intent_id, \
    pending_product_operation_id, pending_tenant_id, pending_installation_id, \
    pending_deployment_id, pending_expected_revision, pending_marked_at \
    FROM public.starring_product_apply_begin_runtime_drain_v2(\
        expected_tenant_id => $1, expected_installation_id => $2, \
        expected_promotion_id => $3, expected_product_revision => $4, \
        expected_payload_digest => $5, expected_principal_id => $6, \
        expected_product_session_digest => $7, session_subject_digest => $8, \
        expected_acting_user_id => $9, expected_discord_application_id => $10, \
        expected_guild_id => $11, expected_capability => $12, \
        expected_authority_revision => $13, expected_authority_payload_digest => $14, \
        expected_authority_observation_digest => $15, expected_authority_observed_at => $16, \
        expected_authority_expires_at => $17, expected_effective_permission_bits => $18, \
        expected_guild_owner => $19, product_request_id => $20, \
        active_idempotency_key_digest => $21, idempotency_key_digest_candidates => $22, \
        idempotency_digest_key_id_candidates => $23, \
        idempotency_digest_key_fingerprint_candidates => $24, \
        idempotency_digest_key_id => $25, semantic_request_digest => $26, \
        new_receipt_id => $27, new_audit_event_id => $28, \
        new_apply_attempt_id => $29, new_deployment_id => $30, \
        proposed_product_operation_id => $31, proposed_drain_intent_id => $32)";
pub(super) const TARGET_ARTIFACT_QUERY: &str = "SELECT schema_version, definition, \
    content_hash, canonical_content_hash \
    FROM public.starring_product_apply_target_artifact_v1($1, $2, $3, $4, $5, $6, $7) \
    LIMIT 2";
pub(super) const FINALIZE_QUERY: &str = "SELECT outcome, resulting_revision, resulting_state, \
    exact_replay, guild_id, deployment_id, desired_target_digest \
    FROM public.starring_product_apply_finalize_v1(\
        expected_tenant_id => $1, expected_installation_id => $2, \
        expected_promotion_id => $3, expected_product_revision => $4, \
        expected_payload_digest => $5, expected_principal_id => $6, \
        expected_product_session_digest => $7, session_subject_digest => $8, \
        expected_acting_user_id => $9, expected_discord_application_id => $10, \
        expected_guild_id => $11, expected_capability => $12, \
        expected_authority_revision => $13, expected_authority_payload_digest => $14, \
        expected_authority_observation_digest => $15, expected_authority_observed_at => $16, \
        expected_authority_expires_at => $17, expected_effective_permission_bits => $18, \
        expected_guild_owner => $19, product_request_id => $20, \
        active_idempotency_key_digest => $21, idempotency_key_digest_candidates => $22, \
        idempotency_digest_key_id_candidates => $23, \
        idempotency_digest_key_fingerprint_candidates => $24, \
        idempotency_digest_key_id => $25, semantic_request_digest => $26, \
        new_receipt_id => $27, new_audit_event_id => $28, \
        new_apply_attempt_id => $29, new_deployment_id => $30, locked_projection => $31, \
        prepared_desired_target_digest => $32, prepared_previous_runtime => $33, \
        prepared_snapshot => $34, prepared_activation_notices => $35)";
