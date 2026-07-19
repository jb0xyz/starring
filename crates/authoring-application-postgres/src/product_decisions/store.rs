use authoring_application::{
    AuthorizedApprovalPreviewV1, AuthorizedApproveProductV1, AuthorizedProductStatusV1,
    CapabilityV1, FreshGuildAuthorityEvidence, ProductApprovalPort, ProductApprovalPreviewV1,
    ProductControlPortError, ProductDecisionProjectionV1, ProductDecisionQueryPort,
    ProductMutationReceiptV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use sqlx::postgres::PgPool;

use super::config::{
    PostgresProductDecisionsConfig, ProductDecisionConfigError, ProductDecisionDigestKeyringV1,
};
use super::digest::approval_digests;
use super::row::{
    approval_guild_from_database, approval_phase_from_database, approval_revision_from_database,
    validate_decision_row, ProductDecisionRow,
};
use crate::ProductDatabaseFailureV1;

#[derive(Clone)]
pub struct PostgresProductDecisions {
    pool: PgPool,
    config: PostgresProductDecisionsConfig,
}

impl PostgresProductDecisions {
    pub fn new(
        pool: PgPool,
        keyring: ProductDecisionDigestKeyringV1,
    ) -> Result<Self, ProductDecisionConfigError> {
        Ok(Self {
            pool,
            config: PostgresProductDecisionsConfig::production(keyring)?,
        })
    }

    pub fn with_config(pool: PgPool, config: PostgresProductDecisionsConfig) -> Self {
        Self { pool, config }
    }
}

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

impl ProductApprovalPort<FreshDiscordAuthorityEvidenceV1> for PostgresProductDecisions {
    async fn approve_payload_bound(
        &self,
        request: AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        validate_approval_evidence(&request)?;
        let digests = approval_digests(self.config.keyring(), &request);
        let evidence = request.evidence();
        let observed_at = evidence.observed_at();
        let expires_at = evidence.expires_at();
        let permission_bits = evidence.effective_permissions_bits().to_string();
        let mut transaction = self.pool.begin().await.map_err(database_backend)?;
        configure_mutation_transaction(&mut transaction, &self.config).await?;
        let outcome = sqlx::query_as::<_, ApprovalOutcomeRow>(
            "SELECT outcome, resulting_revision, resulting_state, exact_replay, guild_id \
             FROM public.starring_product_approve_v1(\
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
             $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)",
        )
        .bind(request.scope().tenant_id().as_str())
        .bind(request.scope().installation_id().as_str())
        .bind(request.command().promotion.promotion_id().as_str())
        .bind(
            i64::try_from(request.command().expected_revision.get()).map_err(|_| {
                ProductControlPortError::Backend(
                    "product revision exceeds PostgreSQL range".to_string(),
                )
            })?,
        )
        .bind(request.command().expected_payload_digest.as_str())
        .bind(request.actor().principal_id().as_str())
        .bind(request.session_fingerprint().as_bytes().as_slice())
        .bind(&digests.session_subject)
        .bind(evidence.acting_user_id().to_string())
        .bind(evidence.discord_application_id().get().to_string())
        .bind(evidence.guild_id().to_string())
        .bind("approve")
        .bind(
            i64::try_from(evidence.installation_authority_revision().get()).map_err(|_| {
                ProductControlPortError::Backend(
                    "authority revision exceeds PostgreSQL range".to_string(),
                )
            })?,
        )
        .bind(evidence.installation_authority_digest())
        .bind(evidence.observation_digest())
        .bind(observed_at)
        .bind(expires_at)
        .bind(permission_bits)
        .bind(evidence.guild_owner())
        .bind(request.request_id().as_str())
        .bind(&digests.active_idempotency)
        .bind(&digests.idempotency_candidates)
        .bind(&digests.idempotency_candidate_key_ids)
        .bind(&digests.idempotency_candidate_key_fingerprints)
        .bind(&digests.active_key_id)
        .bind(&digests.semantic_request)
        .bind(&digests.receipt_id)
        .bind(&digests.audit_event_id)
        .fetch_one(&mut *transaction)
        .await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                transaction.rollback().await.map_err(database_backend)?;
                return Err(database_backend(error));
            }
        };
        if outcome.outcome != "ok" {
            transaction.rollback().await.map_err(database_backend)?;
            return Err(map_approval_outcome(&outcome.outcome));
        }
        let revision = approval_revision_from_database(
            outcome
                .resulting_revision
                .ok_or_else(invalid_approval_result)?,
        )?;
        let phase = approval_phase_from_database(
            outcome
                .resulting_state
                .as_deref()
                .ok_or_else(invalid_approval_result)?,
        )?;
        let guild_id = approval_guild_from_database(
            outcome
                .guild_id
                .as_deref()
                .ok_or_else(invalid_approval_result)?,
        )?;
        if guild_id != request.scope().guild_id() {
            transaction.rollback().await.map_err(database_backend)?;
            return Err(ProductControlPortError::ScopeMismatch);
        }
        let projection = ProductDecisionProjectionV1::from_server_projection(
            request.scope().tenant_id().clone(),
            request.scope().installation_id().clone(),
            guild_id,
            request.command().promotion.promotion_id().clone(),
            revision,
            phase,
        );
        transaction.commit().await.map_err(database_commit)?;
        Ok(ProductMutationReceiptV1::from_server_projection(
            projection,
            outcome.exact_replay,
        ))
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
        let mut transaction = self.pool.begin().await.map_err(database_backend)?;
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

async fn configure_mutation_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &PostgresProductDecisionsConfig,
) -> Result<(), ProductControlPortError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **transaction)
        .await
        .map_err(database_backend)?;
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(config.statement_timeout())
        .execute(&mut **transaction)
        .await
        .map_err(database_backend)?;
    sqlx::query("SELECT pg_catalog.set_config('lock_timeout', $1, true)")
        .bind(config.lock_timeout())
        .execute(&mut **transaction)
        .await
        .map_err(database_backend)?;
    Ok(())
}

fn validate_approval_evidence(
    request: &AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<(), ProductControlPortError> {
    let evidence = request.evidence();
    if evidence.capability() != CapabilityV1::Approve {
        return Err(ProductControlPortError::InvalidState);
    }
    if evidence.tenant_id() != request.scope().tenant_id()
        || evidence.installation_id() != request.scope().installation_id()
        || evidence.guild_id() != request.scope().guild_id()
        || evidence.acting_user_id() != request.scope().acting_user_id()
    {
        return Err(ProductControlPortError::ScopeMismatch);
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ApprovalOutcomeRow {
    outcome: String,
    resulting_revision: Option<i64>,
    resulting_state: Option<String>,
    exact_replay: bool,
    guild_id: Option<String>,
}

fn map_approval_outcome(outcome: &str) -> ProductControlPortError {
    match outcome {
        "not_found" => ProductControlPortError::NotFound,
        "scope_mismatch" => ProductControlPortError::ScopeMismatch,
        "revision_conflict" => ProductControlPortError::RevisionConflict,
        "payload_mismatch" => ProductControlPortError::PayloadMismatch,
        "invalid_state" | "authorization_stale" | "authority_mismatch" => {
            ProductControlPortError::InvalidState
        }
        "self_approval_forbidden" => ProductControlPortError::SelfApprovalForbidden,
        "duplicate_decision" => ProductControlPortError::DuplicateDecision,
        "expired" => ProductControlPortError::Expired,
        "idempotency_conflict" => ProductControlPortError::IdempotencyConflict,
        "idempotency_keyring_incomplete" => ProductControlPortError::Backend(
            "product approval idempotency keyring does not cover live receipts".to_string(),
        ),
        "indeterminate" => ProductControlPortError::Indeterminate(
            "persisted product approval receipt is incomplete".to_string(),
        ),
        _ => invalid_approval_result(),
    }
}

fn database_backend(error: sqlx::Error) -> ProductControlPortError {
    ProductControlPortError::Backend(ProductDatabaseFailureV1::classify(&error).to_string())
}

fn database_commit(error: sqlx::Error) -> ProductControlPortError {
    if matches!(&error, sqlx::Error::Database(_)) {
        database_backend(error)
    } else {
        ProductControlPortError::Indeterminate(
            "product approval commit outcome is unavailable".to_string(),
        )
    }
}

fn invalid_approval_result() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "product approval function returned an invalid result".to_string(),
    )
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
    authority.binding_revision AS authority_binding_revision, \
    authority.binding_fingerprint AS authority_binding_fingerprint, \
    authority.policy_revision AS authority_policy_revision, \
    authority.required_approvals AS authority_required_approvals, \
    authority.activation_ttl_seconds AS authority_activation_ttl_seconds, \
    authority.authority_payload_digest AS authority_payload_digest, \
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
INNER JOIN public.automation_installation_authority_versions AS authority \
    ON authority.tenant_id = installation.tenant_id \
    AND authority.installation_id = installation.installation_id \
    AND authority.revision = installation.current_authority_revision \
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
