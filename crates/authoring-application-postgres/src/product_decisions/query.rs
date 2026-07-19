use authoring_application::{
    AuthorizedApprovalPreviewV1, AuthorizedProductStatusV1, CapabilityV1, ProductApprovalPreviewV1,
    ProductControlPortError, ProductDecisionProjectionV1, ProductDecisionQueryPort,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;

use super::database::database_backend;
use super::row::{validate_decision_row, ProductDecisionRow};
use super::store::PostgresProductDecisions;

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductDecisions {
    async fn load_approval_preview(
        &self,
        request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        let validated = self
            .load_validated(
                request.actor(),
                request.scope(),
                request.evidence(),
                request.promotion(),
            )
            .await?;
        Ok(validated.preview)
    }

    async fn load_product_status(
        &self,
        request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        let validated = self
            .load_validated(
                request.actor(),
                request.scope(),
                request.evidence(),
                request.promotion(),
            )
            .await?;
        Ok(validated.projection)
    }
}

impl PostgresProductDecisions {
    async fn load_validated(
        &self,
        actor: &authoring_application::AuthenticatedActorV1,
        scope: &authoring_application::AuthorizedInstallationScopeV1,
        evidence: &FreshDiscordAuthorityEvidenceV1,
        promotion: &authoring_application::PromotionSelectorV1,
    ) -> Result<super::row::ValidatedDecisionRow, ProductControlPortError> {
        let mut transaction = self
            .pools
            .decision_reader
            .begin()
            .await
            .map_err(database_backend)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(database_backend)?;
        sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
            .bind(self.config.statement_timeout())
            .execute(&mut *transaction)
            .await
            .map_err(database_backend)?;
        let row = sqlx::query_as::<_, ProductDecisionRow>(DECISION_QUERY)
            .bind(promotion.promotion_id().as_str())
            .bind(scope.tenant_id().as_str())
            .bind(scope.installation_id().as_str())
            .bind(scope.guild_id().to_string())
            .bind(actor.principal_id().as_str())
            .bind(actor.session_fingerprint().as_bytes().as_slice())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database_backend)?
            .ok_or(ProductControlPortError::NotFound)?;
        let validated = validate_decision_row(row, scope, evidence, promotion, CapabilityV1::Read)?;
        transaction.commit().await.map_err(database_backend)?;
        Ok(validated)
    }
}

const DECISION_QUERY: &str = "SELECT \
    activation.id AS activation_request_id, \
    activation.tenant_id AS activation_tenant_id, \
    activation.installation_id AS activation_installation_id, \
    activation.guild_id AS activation_guild_id, \
    activation.ruleset_key AS activation_ruleset_key, \
    activation.requester_id AS activation_requester_id, \
    activation.required_approvals AS activation_required_approvals, \
    activation.state AS activation_state, \
    activation.created_at AS activation_created_at, \
    activation.expires_at AS activation_expires_at, \
    activation.promotion_request_digest AS activation_promotion_request_digest, \
    activation.approval_payload_digest AS activation_approval_payload_digest, \
    activation.approval_context AS activation_approval_context, \
    activation.product_revision AS activation_product_revision, \
    (SELECT pg_catalog.count(*) FROM public.activation_request_approvals AS approval \
        WHERE approval.request_id = activation.id) AS approval_count, \
    promotion.tenant_id AS promotion_tenant_id, \
    promotion.stage AS promotion_stage, \
    promotion.request_digest AS promotion_request_digest, \
    promotion.record AS promotion_record, \
    tenant.lifecycle_state AS tenant_lifecycle_state, \
    installation.discord_application_id AS installation_application_id, \
    installation.discord_guild_id AS installation_guild_id, \
    installation.ruleset_key AS installation_ruleset_key, \
    installation.lifecycle_state AS installation_lifecycle_state, \
    installation.current_authority_revision AS installation_current_authority_revision, \
    current_authority.authority_payload_digest AS current_authority_payload_digest, \
    promoted_session.owner_principal_id AS promoted_session_owner_principal_id, \
    promoted_owner.discord_user_id AS promoted_session_owner_discord_user_id, \
    promoted_generation.session_id AS promoted_generation_session_id, \
    promoted_generation.generation AS promoted_generation, \
    promoted_generation.stage AS promoted_generation_stage, \
    promoted_generation.candidate_revision AS promoted_generation_candidate_revision, \
    promoted_generation.candidate_hash AS promoted_generation_candidate_hash, \
    promoted_generation.resource_bindings AS promoted_generation_resource_bindings, \
    promoted_generation.binding_fingerprint AS promoted_generation_binding_fingerprint, \
    historical_authority.binding_revision AS historical_authority_binding_revision, \
    historical_authority.resource_bindings AS historical_authority_resource_bindings, \
    historical_authority.binding_fingerprint \
        AS historical_authority_resource_context_fingerprint, \
    historical_authority.policy_revision AS historical_authority_policy_revision, \
    historical_authority.required_approvals AS historical_authority_required_approvals, \
    historical_authority.activation_ttl_seconds \
        AS historical_authority_activation_ttl_seconds, \
    principal.discord_user_id AS actor_discord_user_id, \
    principal.disabled AS actor_disabled, \
    actor_session.revoked_at AS actor_session_revoked_at, \
    actor_session.idle_expires_at AS actor_session_idle_expires_at, \
    actor_session.absolute_expires_at AS actor_session_absolute_expires_at, \
    deployment.deployment_id AS runtime_deployment_id, \
    deployment.desired_target_digest AS runtime_desired_target_digest, \
    CURRENT_TIMESTAMP AS database_now \
FROM public.activation_requests AS activation \
INNER JOIN public.authoring_promotions AS promotion \
    ON promotion.id = activation.promotion_id \
    AND promotion.tenant_id = activation.tenant_id \
    AND promotion.installation_id = activation.installation_id \
INNER JOIN public.product_tenants AS tenant \
    ON tenant.tenant_id = activation.tenant_id \
INNER JOIN public.automation_installations AS installation \
    ON installation.tenant_id = activation.tenant_id \
    AND installation.installation_id = activation.installation_id \
INNER JOIN public.automation_installation_authority_versions AS current_authority \
    ON current_authority.tenant_id = installation.tenant_id \
    AND current_authority.installation_id = installation.installation_id \
    AND current_authority.revision = installation.current_authority_revision \
LEFT JOIN public.authoring_sessions AS promoted_session \
    ON promoted_session.tenant_id = promotion.tenant_id \
    AND promoted_session.installation_id = promotion.installation_id \
    AND promoted_session.session_id = promotion.record #>> '{intent,authority,session_id}' \
LEFT JOIN public.authoring_session_generations AS promoted_generation \
    ON promoted_generation.tenant_id = promoted_session.tenant_id \
    AND promoted_generation.installation_id = promoted_session.installation_id \
    AND promoted_generation.session_id = promoted_session.session_id \
    AND promoted_generation.generation::TEXT \
        = promotion.record #>> '{intent,authority,session_generation}' \
LEFT JOIN public.product_principals AS promoted_owner \
    ON promoted_owner.principal_id = promoted_session.owner_principal_id \
LEFT JOIN public.automation_installation_authority_versions AS historical_authority \
    ON historical_authority.tenant_id = promoted_generation.tenant_id \
    AND historical_authority.installation_id = promoted_generation.installation_id \
    AND historical_authority.revision = promoted_generation.installation_authority_revision \
INNER JOIN public.product_principals AS principal \
    ON principal.principal_id = $5 \
INNER JOIN public.product_auth_sessions AS actor_session \
    ON actor_session.principal_id = principal.principal_id \
    AND actor_session.session_digest = $6 \
LEFT JOIN public.runtime_deployments AS deployment \
    ON deployment.activation_request_id = activation.id \
    AND deployment.tenant_id = activation.tenant_id \
    AND deployment.installation_id = activation.installation_id \
    AND deployment.promotion_id = activation.promotion_id \
WHERE activation.promotion_id = $1 \
    AND activation.tenant_id = $2 \
    AND activation.installation_id = $3 \
    AND activation.guild_id = $4 \
    AND activation.authority_kind = 'product_authoring' \
    AND actor_session.oauth_state_digest IS NOT NULL";
