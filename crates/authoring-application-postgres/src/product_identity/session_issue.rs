use authoring_application_discord::VerifiedDiscordIdentityV1;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use crate::digest::digest_opaque_session_credential_v1;
use crate::{ProductSecretGenerator, ProductSessionDigestV1};

use super::database::{
    begin_bounded_identity_transaction, identity_database_error, remaining_seconds,
};
use super::principal::{decode_principal, PrincipalUpsertRow, VerifiedIdentityProjection};
use super::store::{PostgresProductIdentityStore, SECRET_INSERT_ATTEMPTS};
use super::{
    ConsumedOAuthFlowV1, CurrentProductPrincipalV1, IssuedProductSessionV1, ProductIdentityError,
};

const ISSUE_SESSION_QUERY: &str = "SELECT * FROM public.starring_product_session_issue_v1(\
     $1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

#[derive(sqlx::FromRow)]
struct SessionIssueRow {
    outcome_code: String,
    principal_id: Option<String>,
    discord_user_id: Option<String>,
    identity_revision: Option<i64>,
    display_profile: Option<Json<Value>>,
    idle_expires_at: Option<DateTime<Utc>>,
    absolute_expires_at: Option<DateTime<Utc>>,
    database_now: Option<DateTime<Utc>>,
}

impl SessionIssueRow {
    fn failure_projection_is_empty(&self) -> bool {
        self.principal_id.is_none()
            && self.discord_user_id.is_none()
            && self.identity_revision.is_none()
            && self.display_profile.is_none()
            && self.idle_expires_at.is_none()
            && self.absolute_expires_at.is_none()
    }

    fn decode_success(
        self,
        identity: VerifiedIdentityProjection<'_>,
        session_digest: ProductSessionDigestV1,
        exact_replay: bool,
    ) -> Result<ValidatedSessionIssueV1, ProductIdentityError> {
        let principal = decode_principal(
            PrincipalUpsertRow {
                principal_id: self.principal_id.ok_or(ProductIdentityError::Invariant)?,
                discord_user_id: self
                    .discord_user_id
                    .ok_or(ProductIdentityError::Invariant)?,
                identity_revision: self
                    .identity_revision
                    .ok_or(ProductIdentityError::Invariant)?,
                display_profile: self
                    .display_profile
                    .ok_or(ProductIdentityError::Invariant)?,
            },
            session_digest,
            self.absolute_expires_at
                .ok_or(ProductIdentityError::Invariant)?,
            ProductIdentityError::Invariant,
        )?;
        let idle_expires_at = self
            .idle_expires_at
            .ok_or(ProductIdentityError::Invariant)?;
        let database_now = self.database_now.ok_or(ProductIdentityError::Invariant)?;
        if principal.discord_user_id() != identity.discord_user_id
            || (!exact_replay && principal.display_name() != identity.display_name)
            || idle_expires_at > principal.absolute_expires_at()
        {
            return Err(ProductIdentityError::Invariant);
        }
        let max_age_seconds = remaining_seconds(database_now, idle_expires_at)
            .filter(|seconds| *seconds <= 1_800)
            .ok_or(ProductIdentityError::Invariant)?;
        Ok(ValidatedSessionIssueV1 {
            principal,
            max_age_seconds,
        })
    }
}

struct ValidatedSessionIssueV1 {
    principal: CurrentProductPrincipalV1,
    max_age_seconds: u32,
}

enum SessionIssueAttemptV1 {
    Success(ValidatedSessionIssueV1),
    DigestConflict,
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
            let first_attempt = self
                .execute_session_issue_attempt(
                    &consumed_flow,
                    identity,
                    &session_digest,
                    &csrf_digest,
                )
                .await;
            let validated = match first_attempt {
                Ok(SessionIssueAttemptV1::Success(validated)) => validated,
                Ok(SessionIssueAttemptV1::DigestConflict) => continue,
                Err(ProductIdentityError::CommitIndeterminate) => match self
                    .execute_session_issue_attempt(
                        &consumed_flow,
                        identity,
                        &session_digest,
                        &csrf_digest,
                    )
                    .await
                {
                    Ok(SessionIssueAttemptV1::Success(validated)) => validated,
                    _ => return Err(ProductIdentityError::CommitIndeterminate),
                },
                Err(error) => return Err(error),
            };
            return Ok(IssuedProductSessionV1 {
                principal: validated.principal,
                session,
                csrf,
                return_path: consumed_flow.return_path,
                max_age_seconds: validated.max_age_seconds,
            });
        }
        Err(ProductIdentityError::SecretGeneration)
    }

    async fn execute_session_issue_attempt(
        &self,
        consumed_flow: &ConsumedOAuthFlowV1,
        identity: VerifiedIdentityProjection<'_>,
        session_digest: &ProductSessionDigestV1,
        csrf_digest: &ProductSessionDigestV1,
    ) -> Result<SessionIssueAttemptV1, ProductIdentityError> {
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let mut transaction =
            begin_bounded_identity_transaction(&self.pools.session_issuer, timeout.as_str())
                .await
                .map_err(identity_database_error)?;
        let row = sqlx::query_as::<_, SessionIssueRow>(ISSUE_SESSION_QUERY)
            .bind(consumed_flow.state_digest.as_bytes().as_slice())
            .bind(&consumed_flow.redirect_uri)
            .bind(&consumed_flow.return_path)
            .bind(consumed_flow.consumed_at)
            .bind(identity.discord_user_id.to_string())
            .bind(identity.display_name)
            .bind(session_digest.as_bytes().as_slice())
            .bind(csrf_digest.as_bytes().as_slice())
            .bind(self.config.lifetimes().session_idle().as_secs_f64())
            .bind(self.config.lifetimes().session_absolute().as_secs_f64())
            .fetch_one(&mut *transaction)
            .await;
        let row = match row {
            Ok(row) => row,
            Err(error) => {
                let failure = identity_database_error(error);
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(failure);
            }
        };
        match row.outcome_code.as_str() {
            "issued" | "exact_replay" => {
                let exact_replay = row.outcome_code == "exact_replay";
                let validated = row.decode_success(identity, session_digest.clone(), exact_replay);
                let validated = match validated {
                    Ok(validated) => validated,
                    Err(error) => {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(error);
                    }
                };
                if exact_replay {
                    let _ = transaction.rollback().await;
                } else {
                    self.commit_issued_session(transaction).await?;
                }
                Ok(SessionIssueAttemptV1::Success(validated))
            }
            "digest_conflict" => {
                let valid_shape = row.failure_projection_is_empty() && row.database_now.is_some();
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                if valid_shape {
                    Ok(SessionIssueAttemptV1::DigestConflict)
                } else {
                    Err(ProductIdentityError::Invariant)
                }
            }
            "flow_invalid_or_consumed" => {
                let valid_shape = row.failure_projection_is_empty();
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                if valid_shape {
                    Err(ProductIdentityError::FlowInvalidOrConsumed)
                } else {
                    Err(ProductIdentityError::Invariant)
                }
            }
            "principal_disabled" => {
                let valid_shape = row.failure_projection_is_empty();
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                if valid_shape {
                    Err(ProductIdentityError::PrincipalDisabled)
                } else {
                    Err(ProductIdentityError::Invariant)
                }
            }
            _ => {
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                Err(ProductIdentityError::Invariant)
            }
        }
    }

    async fn commit_issued_session(
        &self,
        transaction: Transaction<'_, Postgres>,
    ) -> Result<(), ProductIdentityError> {
        #[cfg(test)]
        if self
            .session_issue_rollback_before_ack_loss
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            transaction
                .rollback()
                .await
                .map_err(identity_database_error)?;
            return Err(ProductIdentityError::CommitIndeterminate);
        }
        transaction
            .commit()
            .await
            .map_err(|_| ProductIdentityError::CommitIndeterminate)?;
        #[cfg(test)]
        {
            use std::sync::atomic::Ordering;
            use std::time::Duration;

            let delay_millis = self
                .session_issue_commit_ack_loss_delay_millis
                .swap(0, Ordering::SeqCst);
            if delay_millis != 0 {
                tokio::time::sleep(Duration::from_millis(delay_millis)).await;
                if self
                    .session_issue_close_pool_after_ack_loss
                    .swap(false, Ordering::SeqCst)
                {
                    self.pools.session_issuer.close().await;
                }
                return Err(ProductIdentityError::CommitIndeterminate);
            }
        }
        Ok(())
    }
}
