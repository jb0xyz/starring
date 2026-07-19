use chrono::{DateTime, Utc};

use crate::digest::digest_opaque_session_credential_v1;
use crate::ProductSecretGenerator;

use super::database::{
    database_time, oauth_database_error, remaining_seconds, set_statement_timeout, unique_violation,
};
use super::store::{PostgresProductIdentityStore, SECRET_INSERT_ATTEMPTS};
use super::{ConsumedOAuthFlowV1, OAuthFlowError, OAuthFlowIssueV1};

#[derive(sqlx::FromRow)]
struct OAuthFlowInsertRow {
    expires_at: DateTime<Utc>,
    database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct OAuthFlowClaimRow {
    redirect_uri: String,
    return_path: String,
    expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct OAuthFlowConsumeRow {
    consumed_at: DateTime<Utc>,
}

impl<G> PostgresProductIdentityStore<G>
where
    G: ProductSecretGenerator,
{
    pub async fn create_oauth_flow(
        &self,
        return_path: &str,
    ) -> Result<OAuthFlowIssueV1, OAuthFlowError> {
        if !self.config.allows_return_path(return_path) {
            return Err(OAuthFlowError::InvalidRequest);
        }
        for _ in 0..SECRET_INSERT_ATTEMPTS {
            let (state, browser_nonce) = self.generate_distinct_pair()?;
            let state_digest = digest_opaque_session_credential_v1(state.expose_secret())
                .map_err(|_| OAuthFlowError::SecretGeneration)?;
            let nonce_digest = digest_opaque_session_credential_v1(browser_nonce.expose_secret())
                .map_err(|_| OAuthFlowError::SecretGeneration)?;
            let mut transaction = self.pool.begin().await.map_err(oauth_database_error)?;
            set_statement_timeout(
                &mut transaction,
                self.config.lifetimes().authentication().statement_timeout(),
            )
            .await
            .map_err(oauth_database_error)?;
            let result = sqlx::query_as::<_, OAuthFlowInsertRow>(
                "WITH flow_clock AS (SELECT pg_catalog.clock_timestamp() AS created_at) \
                 INSERT INTO public.product_oauth_flows \
                 (state_digest, browser_nonce_digest, redirect_uri, return_path, \
                  created_at, expires_at) \
                 SELECT $1, $2, $3, $4, created_at, \
                  created_at + pg_catalog.make_interval(secs => $5::DOUBLE PRECISION) FROM flow_clock \
                 RETURNING expires_at, created_at AS database_now",
            )
            .bind(state_digest.as_bytes().as_slice())
            .bind(nonce_digest.as_bytes().as_slice())
            .bind(self.config.redirect_uri())
            .bind(return_path)
            .bind(self.config.lifetimes().oauth_flow().as_secs_f64())
            .fetch_one(&mut *transaction)
            .await;
            match result {
                Ok(row) => {
                    let Some(max_age_seconds) = remaining_seconds(row.database_now, row.expires_at)
                    else {
                        transaction.rollback().await.map_err(oauth_database_error)?;
                        return Err(OAuthFlowError::Invariant);
                    };
                    transaction
                        .commit()
                        .await
                        .map_err(|_| OAuthFlowError::CommitIndeterminate)?;
                    return Ok(OAuthFlowIssueV1 {
                        state,
                        browser_nonce,
                        redirect_uri: self.config.redirect_uri().to_string(),
                        return_path: return_path.to_string(),
                        expires_at: row.expires_at,
                        max_age_seconds,
                    });
                }
                Err(error) if unique_violation(&error) => {
                    transaction.rollback().await.map_err(oauth_database_error)?;
                }
                Err(error) => {
                    let failure = oauth_database_error(error);
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    return Err(failure);
                }
            }
        }
        Err(OAuthFlowError::SecretGeneration)
    }

    pub async fn consume_oauth_flow(
        &self,
        state: &str,
        browser_nonce: &str,
    ) -> Result<ConsumedOAuthFlowV1, OAuthFlowError> {
        let state_digest = digest_opaque_session_credential_v1(state)
            .map_err(|_| OAuthFlowError::InvalidOrConsumed)?;
        let nonce_digest = digest_opaque_session_credential_v1(browser_nonce)
            .map_err(|_| OAuthFlowError::InvalidOrConsumed)?;
        if state_digest == nonce_digest {
            return Err(OAuthFlowError::InvalidOrConsumed);
        }
        let mut transaction = self.pool.begin().await.map_err(oauth_database_error)?;
        set_statement_timeout(
            &mut transaction,
            self.config.lifetimes().authentication().statement_timeout(),
        )
        .await
        .map_err(oauth_database_error)?;
        let claim = sqlx::query_as::<_, OAuthFlowClaimRow>(
            "SELECT redirect_uri, return_path, expires_at FROM public.product_oauth_flows \
             WHERE state_digest = $1 AND browser_nonce_digest = $2 \
             AND redirect_uri = $3 AND return_path = ANY($4::TEXT[]) \
             AND expires_at <= created_at + INTERVAL '10 minutes' \
             AND consumed_at IS NULL FOR UPDATE",
        )
        .bind(state_digest.as_bytes().as_slice())
        .bind(nonce_digest.as_bytes().as_slice())
        .bind(self.config.redirect_uri())
        .bind(self.config.allowed_return_paths())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(oauth_database_error)?;
        let Some(claim) = claim else {
            transaction.rollback().await.map_err(oauth_database_error)?;
            return Err(OAuthFlowError::InvalidOrConsumed);
        };
        let database_now = match database_time(&mut transaction).await {
            Ok(database_now) => database_now,
            Err(error) => {
                let failure = oauth_database_error(error);
                transaction.rollback().await.map_err(oauth_database_error)?;
                return Err(failure);
            }
        };
        if database_now >= claim.expires_at {
            transaction.rollback().await.map_err(oauth_database_error)?;
            return Err(OAuthFlowError::InvalidOrConsumed);
        }
        let row = sqlx::query_as::<_, OAuthFlowConsumeRow>(
            "UPDATE public.product_oauth_flows \
             SET consumed_at = $2, terminal_result_code = 'callback_claimed' \
             WHERE state_digest = $1 AND consumed_at IS NULL \
             RETURNING consumed_at",
        )
        .bind(state_digest.as_bytes().as_slice())
        .bind(database_now)
        .fetch_one(&mut *transaction)
        .await;
        let row = match row {
            Ok(row) => row,
            Err(error) => {
                let failure = oauth_database_error(error);
                transaction.rollback().await.map_err(oauth_database_error)?;
                return Err(failure);
            }
        };
        transaction
            .commit()
            .await
            .map_err(|_| OAuthFlowError::CommitIndeterminate)?;
        Ok(ConsumedOAuthFlowV1 {
            state_digest,
            redirect_uri: claim.redirect_uri,
            return_path: claim.return_path,
            consumed_at: row.consumed_at,
        })
    }
}
