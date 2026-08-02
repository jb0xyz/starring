pub(super) const DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_product_deployment_status_reader_database_identity_v1()";
pub(super) const STATUS_FUNCTION: &str = "public.starring_product_deployment_status_read_v3(text,text,text,text,text,text,text,text,bytea)";
pub(super) const STATUS_ARGUMENTS: &str = "expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea";
pub(super) const STATUS_RESULT: &str = "TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, attestation_record_format_version smallint, attestation_serving_lease_duration_nanos bigint, attestation_convergence_attempt_no bigint, deployment_last_controller_id text, v2_evidence_state text, v2_operation_id text, v2_intent_fingerprint text, v2_certification_intent_bytes bytea, v2_request_digest text, v2_request_bytes bytea, v2_live_attestation_bytes bytea, v2_must_commit_before timestamp with time zone, v2_route_admission jsonb, v2_certified_snapshot jsonb)";
pub(super) const TOPOLOGY_QUERY: &str = "SELECT \
    public.starring_product_deployment_status_reader_database_identity_v1(), \
    current_database()::TEXT, current_user::TEXT, session_user::TEXT";
pub(super) const STATUS_QUERY: &str = "SELECT request_outcome, deployment_projection, \
    activation_projection, promotion_projection, tenant_lifecycle_state, \
    installation_projection, historical_authority_projection, \
    current_authority_projection, active_target_version, artifact_projection, \
    attestation_projection, serving_projection, database_now, \
    attestation_record_format_version, attestation_serving_lease_duration_nanos, \
    attestation_convergence_attempt_no, deployment_last_controller_id, \
    v2_evidence_state, v2_operation_id, \
    v2_intent_fingerprint, v2_certification_intent_bytes, v2_request_digest, \
    v2_request_bytes, v2_live_attestation_bytes, v2_must_commit_before, \
    v2_route_admission, v2_certified_snapshot \
    FROM public.starring_product_deployment_status_read_v3(\
        expected_deployment_id => $1, expected_promotion_id => $2, \
        expected_desired_target_digest => $3, expected_tenant_id => $4, \
        expected_installation_id => $5, expected_guild_id => $6, \
        expected_principal_id => $7, expected_acting_discord_user_id => $8, \
        expected_product_session_digest => $9) \
    LIMIT 2";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_keeps_the_exact_external_surface() {
        assert_eq!(STATUS_ARGUMENTS.matches(',').count() + 1, 9);
        assert_eq!(STATUS_RESULT.matches(',').count() + 1, 27);
        assert_eq!(STATUS_RESULT.matches(" jsonb").count(), 11);
        assert_eq!(STATUS_RESULT.matches(" bytea").count(), 3);
        assert_eq!(STATUS_RESULT.matches("attempt_no bigint").count(), 1);
        assert_eq!(STATUS_RESULT.matches("v2_").count(), 10);
        assert!(STATUS_QUERY.contains("FROM public.starring_product_deployment_status_read_v3("));
        assert!(STATUS_QUERY.ends_with("LIMIT 2"));
    }
}
