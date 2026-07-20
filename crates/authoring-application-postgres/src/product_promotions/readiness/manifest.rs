use crate::database_capability::{ScopedFunctionContractV1, ScopedRelationContractV1};

pub(super) const DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_product_promotion_executor_database_identity_v1()";
pub(super) const REPLAY_FUNCTION: &str = "public.starring_product_promotion_replay_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,bigint,text,text[],text[],text[])";
pub(super) const PREPARE_FUNCTION: &str = "public.starring_product_promotion_prepare_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bytea,text,bigint,bigint,text,text,text,text,jsonb,jsonb,text,text,text[],text[],text[],text,text,text,text)";
pub(super) const PUBLISH_FUNCTION: &str = "public.starring_product_promotion_publish_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)";
pub(super) const APPROVAL_ENVIRONMENT_FUNCTION: &str = "public.starring_product_promotion_approval_environment_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text)";
pub(super) const ACTIVATION_LINK_FUNCTION: &str = "public.starring_product_promotion_activation_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,bigint,text,text,jsonb)";
pub(super) const REPAIR_LINK_FUNCTION: &str = "public.starring_product_promotion_repair_link_v1(text,text,text,bytea,text,text,text,text,bigint,text,text,timestamp with time zone,timestamp with time zone,text,boolean,text,text,text,bytea,jsonb,text,text,text[],text[],text[],text,text,text,text)";
pub(super) const KEYRING_COVERAGE_FUNCTION: &str =
    "public.starring_product_promotion_keyring_coverage_v1(text[],text[])";
pub(super) const REPLAY_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, observed_current_authority_revision bigint, observed_current_authority_payload_digest text, authority_observation_digest text, authority_observed_at timestamp with time zone, authority_expires_at timestamp with time zone, effective_permission_bits text, guild_owner boolean, expected_promotion_id text, expected_session_id text, expected_generation bigint, semantic_request_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[]";
pub(super) const PREPARE_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, observed_current_authority_revision bigint, observed_current_authority_payload_digest text, authority_observation_digest text, authority_observed_at timestamp with time zone, authority_expires_at timestamp with time zone, effective_permission_bits text, guild_owner boolean, product_request_id text, session_subject_digest bytea, expected_session_id text, expected_generation bigint, expected_candidate_revision bigint, expected_candidate_hash text, expected_binding_fingerprint text, expected_promotion_id text, expected_promotion_request_digest text, prepared_promotion_intent jsonb, product_admission_payload jsonb, product_admission_digest text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text";
pub(super) const STAGE_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, observed_current_authority_revision bigint, observed_current_authority_payload_digest text, authority_observation_digest text, authority_observed_at timestamp with time zone, authority_expires_at timestamp with time zone, effective_permission_bits text, guild_owner boolean, expected_promotion_id text, expected_promotion_revision bigint, expected_promotion_request_digest text, expected_admission_digest text";
pub(super) const ACTIVATION_LINK_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, observed_current_authority_revision bigint, observed_current_authority_payload_digest text, authority_observation_digest text, authority_observed_at timestamp with time zone, authority_expires_at timestamp with time zone, effective_permission_bits text, guild_owner boolean, expected_promotion_id text, expected_promotion_revision bigint, expected_promotion_request_digest text, expected_admission_digest text, activation_proposal jsonb";
pub(super) const REPAIR_LINK_ARGUMENTS: &str = "expected_tenant_id text, expected_installation_id text, expected_principal_id text, expected_product_session_digest bytea, expected_acting_user_id text, expected_discord_application_id text, expected_guild_id text, expected_capability text, observed_current_authority_revision bigint, observed_current_authority_payload_digest text, authority_observation_digest text, authority_observed_at timestamp with time zone, authority_expires_at timestamp with time zone, effective_permission_bits text, guild_owner boolean, expected_promotion_id text, expected_promotion_request_digest text, recovery_product_request_id text, recovery_session_subject_digest bytea, recovery_admission_payload jsonb, recovery_admission_digest text, active_idempotency_key_digest text, idempotency_key_digest_candidates text[], idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[], idempotency_digest_key_id text, semantic_request_digest text, new_receipt_id text, new_audit_event_id text";
pub(super) const KEYRING_COVERAGE_ARGUMENTS: &str = "idempotency_digest_key_id_candidates text[], idempotency_digest_key_fingerprint_candidates text[]";
pub(super) const REPLAY_RESULT: &str = "TABLE(outcome_code text, promotion_record jsonb, admission_evidence jsonb, admission_digest text, receipt_projection jsonb, audit_evidence_projection jsonb, database_now timestamp with time zone)";
pub(super) const PREPARE_RESULT: &str = "TABLE(outcome_code text, promotion_record jsonb, admission_evidence jsonb, admission_digest text, database_now timestamp with time zone)";
pub(super) const PUBLISH_RESULT: &str = "TABLE(outcome_code text, publication_projection jsonb, promotion_record jsonb, database_now timestamp with time zone)";
pub(super) const APPROVAL_ENVIRONMENT_RESULT: &str = "TABLE(outcome_code text, promotion_record jsonb, historical_binding_revision bigint, historical_resource_bindings jsonb, historical_binding_fingerprint text, active_version bigint, active_content_hash text, target_artifact_projection jsonb, database_now timestamp with time zone)";
pub(super) const FINAL_RESULT: &str = "TABLE(outcome_code text, promotion_record jsonb, admission_evidence jsonb, admission_digest text, activation_projection jsonb, receipt_projection jsonb, audit_evidence_projection jsonb, database_now timestamp with time zone)";
pub(super) const KEYRING_COVERAGE_RESULT: &str = "TABLE(outcome_code text)";
pub(super) const FUNCTIONS: [ScopedFunctionContractV1<'static>; 8] = [
    ScopedFunctionContractV1::scalar(DATABASE_IDENTITY_FUNCTION, "text"),
    ScopedFunctionContractV1::set_plpgsql_named(
        REPLAY_FUNCTION,
        REPLAY_RESULT,
        1.0,
        REPLAY_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        PREPARE_FUNCTION,
        PREPARE_RESULT,
        1.0,
        PREPARE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        PUBLISH_FUNCTION,
        PUBLISH_RESULT,
        1.0,
        STAGE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        APPROVAL_ENVIRONMENT_FUNCTION,
        APPROVAL_ENVIRONMENT_RESULT,
        1.0,
        STAGE_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        ACTIVATION_LINK_FUNCTION,
        FINAL_RESULT,
        1.0,
        ACTIVATION_LINK_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_plpgsql_named(
        REPAIR_LINK_FUNCTION,
        FINAL_RESULT,
        1.0,
        REPAIR_LINK_ARGUMENTS,
    ),
    ScopedFunctionContractV1::set_named(
        KEYRING_COVERAGE_FUNCTION,
        KEYRING_COVERAGE_RESULT,
        1.0,
        KEYRING_COVERAGE_ARGUMENTS,
    ),
];
pub(super) const RELATIONS: [ScopedRelationContractV1<'static>; 18] = [
    ScopedRelationContractV1::ordinary_without_rls("public.product_control_plane_identity"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_principals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_auth_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_tenants"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_installations"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.automation_installation_authority_versions",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_sessions"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_session_generations"),
    ScopedRelationContractV1::ordinary_without_rls("public.authoring_promotions"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_heads"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_versions"),
    ScopedRelationContractV1::ordinary_without_rls("public.automation_ruleset_activations"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_requests"),
    ScopedRelationContractV1::ordinary_without_rls("public.activation_request_approvals"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipts"),
    ScopedRelationContractV1::ordinary_without_rls(
        "public.product_action_receipt_idempotency_aliases",
    ),
    ScopedRelationContractV1::ordinary_without_rls("public.product_audit_events"),
    ScopedRelationContractV1::ordinary_without_rls("public.product_action_receipt_audit_evidence"),
];
pub(super) const TOPOLOGY_QUERY: &str = "SELECT \
    public.starring_product_promotion_executor_database_identity_v1(), \
    current_database()::TEXT, current_user::TEXT, session_user::TEXT";
pub(super) const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.promotion.digest-key-fingerprint.v1";
pub(super) const PROBE_SESSION_DIGEST: [u8; 32] = [59_u8; 32];
pub(super) const PROBE_SUBJECT_DIGEST: [u8; 32] = [101_u8; 32];
