use authoring_application_discord::VerifiedDiscordIdentityV1;
use chrono::{DateTime, Utc};

use crate::digest::digest_opaque_session_credential_v1;
use crate::ProductSecretGenerator;

use super::database::{
    database_time, identity_database_error, oauth_flow_constraint, remaining_seconds,
    set_statement_timeout, unique_violation,
};
use super::principal::{decode_principal, upsert_principal, VerifiedIdentityProjection};
use super::store::{PostgresProductIdentityStore, SECRET_INSERT_ATTEMPTS};
use super::{
    ConsumedOAuthFlowV1, IssuedProductSessionV1, PostgresProductIdentityConfig,
    ProductIdentityError,
};

#[derive(sqlx::FromRow)]
struct ConsumedOAuthFlowValidationRow {
    redirect_uri: String,
    return_path: String,
    consumed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct SessionInsertRow {
    idle_expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    database_now: DateTime<Utc>,
}

impl<G> PostgresProductIdentityStore<G>
where
    G: ProductSecretGenerator,
{
    pub async fn issue_product_session(
        &self,
        consumed_flow: ConsumedOAuthFlowV1,
        identity: VerifiedDiscordIdentityV1,
    ) -> Result<IssuedProductSessionV1, ProductIdentityError> {
        let identity = VerifiedIdentityProjection::from_capability(&identity);
        self.issue_product_session_core(consumed_flow, identity)
            .await
    }

    pub(super) async fn issue_product_session_core(
        &self,
        consumed_flow: ConsumedOAuthFlowV1,
        identity: VerifiedIdentityProjection<'_>,
    ) -> Result<IssuedProductSessionV1, ProductIdentityError> {
        if consumed_flow.redirect_uri != self.config.redirect_uri()
            || !self.config.allows_return_path(&consumed_flow.return_path)
        {
            return Err(ProductIdentityError::FlowInvalidOrConsumed);
        }
        for _ in 0..SECRET_INSERT_ATTEMPTS {
            let (session, csrf) = self.generate_distinct_pair()?;
            let session_digest = digest_opaque_session_credential_v1(session.expose_secret())
                .map_err(|_| ProductIdentityError::SecretGeneration)?;
            let csrf_digest = digest_opaque_session_credential_v1(csrf.expose_secret())
                .map_err(|_| ProductIdentityError::SecretGeneration)?;
            if session_digest == consumed_flow.state_digest
                || csrf_digest == consumed_flow.state_digest
            {
                continue;
            }
            let mut transaction = self.pool.begin().await.map_err(identity_database_error)?;
            set_statement_timeout(
                &mut transaction,
                self.config.lifetimes().authentication().statement_timeout(),
            )
            .await
            .map_err(identity_database_error)?;
            if let Err(error) =
                validate_consumed_flow(&mut transaction, &consumed_flow, &self.config).await
            {
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(error);
            }
            let principal = match upsert_principal(&mut transaction, identity).await {
                Ok(principal) => principal,
                Err(error) => {
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    return Err(error);
                }
            };
            let inserted = sqlx::query_as::<_, SessionInsertRow>(
                "WITH issue_clock AS (SELECT pg_catalog.clock_timestamp() AS issued_at) \
                 INSERT INTO public.product_auth_sessions \
                 (session_digest, principal_id, csrf_digest, oauth_state_digest, \
                  authenticated_at, created_at, last_seen_at, idle_expires_at, \
                  absolute_expires_at) \
                 SELECT $1, $2, $3, $4, issued_at, issued_at, issued_at, \
                  issued_at + pg_catalog.make_interval(secs => $5::DOUBLE PRECISION), \
                  issued_at + pg_catalog.make_interval(secs => $6::DOUBLE PRECISION) FROM issue_clock \
                 RETURNING idle_expires_at, absolute_expires_at, created_at AS database_now",
            )
            .bind(session_digest.as_bytes().as_slice())
            .bind(&principal.principal_id)
            .bind(csrf_digest.as_bytes().as_slice())
            .bind(consumed_flow.state_digest.as_bytes().as_slice())
            .bind(self.config.lifetimes().session_idle().as_secs_f64())
            .bind(self.config.lifetimes().session_absolute().as_secs_f64())
            .fetch_one(&mut *transaction)
            .await;
            let inserted = match inserted {
                Ok(inserted) => inserted,
                Err(error) if oauth_flow_constraint(&error) => {
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    return Err(ProductIdentityError::FlowInvalidOrConsumed);
                }
                Err(error) if unique_violation(&error) => {
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    continue;
                }
                Err(error) => {
                    let failure = identity_database_error(error);
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    return Err(failure);
                }
            };
            let principal = match decode_principal(
                principal,
                session_digest,
                inserted.absolute_expires_at,
                ProductIdentityError::Invariant,
            ) {
                Ok(principal) => principal,
                Err(error) => {
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    return Err(error);
                }
            };
            let cookie_expires_at = inserted.idle_expires_at.min(inserted.absolute_expires_at);
            let Some(max_age_seconds) = remaining_seconds(inserted.database_now, cookie_expires_at)
            else {
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(ProductIdentityError::Invariant);
            };
            transaction
                .commit()
                .await
                .map_err(|_| ProductIdentityError::CommitIndeterminate)?;
            return Ok(IssuedProductSessionV1 {
                principal,
                session,
                csrf,
                return_path: consumed_flow.return_path,
                max_age_seconds,
            });
        }
        Err(ProductIdentityError::SecretGeneration)
    }
}

async fn validate_consumed_flow(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    consumed_flow: &ConsumedOAuthFlowV1,
    config: &PostgresProductIdentityConfig,
) -> Result<(), ProductIdentityError> {
    let persisted = sqlx::query_as::<_, ConsumedOAuthFlowValidationRow>(
        "SELECT redirect_uri, return_path, consumed_at, expires_at \
         FROM public.product_oauth_flows WHERE state_digest = $1 \
         AND terminal_result_code = 'callback_claimed' \
         AND expires_at <= created_at + INTERVAL '10 minutes' \
         AND NOT EXISTS (SELECT 1 FROM public.product_auth_sessions \
          WHERE oauth_state_digest = $1) FOR SHARE",
    )
    .bind(consumed_flow.state_digest.as_bytes().as_slice())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(identity_database_error)?
    .ok_or(ProductIdentityError::FlowInvalidOrConsumed)?;
    let database_now = database_time(transaction)
        .await
        .map_err(identity_database_error)?;
    if persisted.redirect_uri != consumed_flow.redirect_uri
        || persisted.redirect_uri != config.redirect_uri()
        || persisted.return_path != consumed_flow.return_path
        || !config.allows_return_path(&persisted.return_path)
        || persisted.consumed_at != consumed_flow.consumed_at
        || database_now >= persisted.expires_at
    {
        return Err(ProductIdentityError::FlowInvalidOrConsumed);
    }
    Ok(())
}
