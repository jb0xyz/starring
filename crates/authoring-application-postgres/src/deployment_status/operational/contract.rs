pub(super) const DATABASE_IDENTITY_FUNCTION: &str =
    "public.starring_product_deployment_status_reader_database_identity_v2()";
pub(super) const STATUS_FUNCTION: &str = "public.starring_product_deployment_status_read_v2(text,text,text,text,text,text,text,text,bytea)";
pub(super) const STATUS_ARGUMENTS: &str = "expected_deployment_id text, expected_promotion_id text, expected_desired_target_digest text, expected_tenant_id text, expected_installation_id text, expected_guild_id text, expected_principal_id text, expected_acting_discord_user_id text, expected_product_session_digest bytea";
pub(super) const STATUS_RESULT: &str = "TABLE(request_outcome text, deployment_projection jsonb, activation_projection jsonb, promotion_projection jsonb, tenant_lifecycle_state text, installation_projection jsonb, historical_authority_projection jsonb, current_authority_projection jsonb, active_target_version bigint, artifact_projection jsonb, attestation_projection jsonb, serving_projection jsonb, database_now timestamp with time zone, deployment_convergence_attempt_no bigint, deployment_last_failure_attempt_no bigint, attestation_convergence_attempt_no bigint)";
pub(super) const TOPOLOGY_QUERY: &str = "SELECT \
    public.starring_product_deployment_status_reader_database_identity_v2(), \
    current_database()::TEXT, current_user::TEXT, session_user::TEXT";
pub(super) const STATUS_QUERY: &str = "SELECT request_outcome, deployment_projection, \
    activation_projection, promotion_projection, tenant_lifecycle_state, \
    installation_projection, historical_authority_projection, \
    current_authority_projection, active_target_version, artifact_projection, \
    attestation_projection, serving_projection, database_now, \
    deployment_convergence_attempt_no, deployment_last_failure_attempt_no, \
    attestation_convergence_attempt_no \
    FROM public.starring_product_deployment_status_read_v2(\
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
    fn operational_contract_appends_only_attempt_evidence() {
        assert_eq!(STATUS_ARGUMENTS.matches(',').count() + 1, 9);
        assert_eq!(STATUS_RESULT.matches(" jsonb").count(), 9);
        assert_eq!(STATUS_RESULT.matches("attempt_no bigint").count(), 3);
        assert!(STATUS_QUERY.ends_with("LIMIT 2"));
    }
}
