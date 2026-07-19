use chrono::{DateTime, Utc};

use crate::digest::digest_opaque_session_credential_v1;
use crate::ProductSecretGenerator;

use super::database::{
    begin_bounded_identity_transaction, oauth_database_error, remaining_seconds,
};
use super::store::{PostgresProductIdentityStore, SECRET_INSERT_ATTEMPTS};
use super::{ConsumedOAuthFlowV1, OAuthFlowError, OAuthFlowIssueV1};

const CREATE_FLOW_QUERY: &str =
    "SELECT * FROM public.starring_product_oauth_flow_create_v1($1, $2, $3, $4, $5)";
const CONSUME_FLOW_QUERY: &str =
    "SELECT * FROM public.starring_product_oauth_flow_consume_v1($1, $2, $3, $4)";

#[derive(sqlx::FromRow)]
struct OAuthFlowCreateRow {
    outcome_code: String,
    redirect_uri: Option<String>,
    return_path: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    database_now: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct OAuthFlowConsumeRow {
    outcome_code: String,
    redirect_uri: Option<String>,
    return_path: Option<String>,
    consumed_at: Option<DateTime<Utc>>,
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
            let timeout = self.config.lifetimes().authentication().statement_timeout();
            let mut transaction =
                begin_bounded_identity_transaction(&self.pools.oauth_flow_writer, timeout.as_str())
                    .await
                    .map_err(oauth_database_error)?;
            let row = sqlx::query_as::<_, OAuthFlowCreateRow>(CREATE_FLOW_QUERY)
                .bind(state_digest.as_bytes().as_slice())
                .bind(nonce_digest.as_bytes().as_slice())
                .bind(self.config.redirect_uri())
                .bind(return_path)
                .bind(self.config.lifetimes().oauth_flow().as_secs_f64())
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
            match row.outcome_code.as_str() {
                "created" | "exact_replay" => {
                    let valid_projection = row.redirect_uri.as_deref()
                        == Some(self.config.redirect_uri())
                        && row.return_path.as_deref() == Some(return_path);
                    let Some(expires_at) = row.expires_at else {
                        transaction.rollback().await.map_err(oauth_database_error)?;
                        return Err(OAuthFlowError::Invariant);
                    };
                    let Some(database_now) = row.database_now else {
                        transaction.rollback().await.map_err(oauth_database_error)?;
                        return Err(OAuthFlowError::Invariant);
                    };
                    let Some(max_age_seconds) = remaining_seconds(database_now, expires_at) else {
                        transaction.rollback().await.map_err(oauth_database_error)?;
                        return Err(OAuthFlowError::Invariant);
                    };
                    if !valid_projection || max_age_seconds > 600 {
                        transaction.rollback().await.map_err(oauth_database_error)?;
                        return Err(OAuthFlowError::Invariant);
                    }
                    transaction
                        .commit()
                        .await
                        .map_err(|_| OAuthFlowError::CommitIndeterminate)?;
                    return Ok(OAuthFlowIssueV1 {
                        state,
                        browser_nonce,
                        redirect_uri: row.redirect_uri.expect("validated redirect URI exists"),
                        return_path: row.return_path.expect("validated return path exists"),
                        expires_at,
                        max_age_seconds,
                    });
                }
                "digest_conflict" => {
                    let collision_shape = row.redirect_uri.is_none()
                        && row.return_path.is_none()
                        && row.expires_at.is_none()
                        && row.database_now.is_some();
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    if !collision_shape {
                        return Err(OAuthFlowError::Invariant);
                    }
                }
                "invalid_request" => {
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    return Err(OAuthFlowError::Invariant);
                }
                _ => {
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    return Err(OAuthFlowError::Invariant);
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
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let mut transaction =
            begin_bounded_identity_transaction(&self.pools.oauth_flow_writer, timeout.as_str())
                .await
                .map_err(oauth_database_error)?;
        let row = sqlx::query_as::<_, OAuthFlowConsumeRow>(CONSUME_FLOW_QUERY)
            .bind(state_digest.as_bytes().as_slice())
            .bind(nonce_digest.as_bytes().as_slice())
            .bind(self.config.redirect_uri())
            .bind(self.config.allowed_return_paths())
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
        match row.outcome_code.as_str() {
            "claimed" => {
                let Some(redirect_uri) = row.redirect_uri else {
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    return Err(OAuthFlowError::Invariant);
                };
                let Some(return_path) = row.return_path else {
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    return Err(OAuthFlowError::Invariant);
                };
                let Some(consumed_at) = row.consumed_at else {
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    return Err(OAuthFlowError::Invariant);
                };
                if redirect_uri != self.config.redirect_uri()
                    || !self.config.allows_return_path(&return_path)
                {
                    transaction.rollback().await.map_err(oauth_database_error)?;
                    return Err(OAuthFlowError::Invariant);
                }
                transaction
                    .commit()
                    .await
                    .map_err(|_| OAuthFlowError::CommitIndeterminate)?;
                Ok(ConsumedOAuthFlowV1 {
                    state_digest,
                    redirect_uri,
                    return_path,
                    consumed_at,
                })
            }
            "invalid_or_consumed" => {
                let invalid_shape = row.redirect_uri.is_none()
                    && row.return_path.is_none()
                    && row.consumed_at.is_none();
                transaction.rollback().await.map_err(oauth_database_error)?;
                if invalid_shape {
                    Err(OAuthFlowError::InvalidOrConsumed)
                } else {
                    Err(OAuthFlowError::Invariant)
                }
            }
            _ => {
                transaction.rollback().await.map_err(oauth_database_error)?;
                Err(OAuthFlowError::Invariant)
            }
        }
    }
}
