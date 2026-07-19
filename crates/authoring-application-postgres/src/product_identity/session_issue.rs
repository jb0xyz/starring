use authoring_application_discord::VerifiedDiscordIdentityV1;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::types::Json;

use crate::digest::digest_opaque_session_credential_v1;
use crate::ProductSecretGenerator;

use super::database::{
    begin_bounded_identity_transaction, identity_database_error, remaining_seconds,
};
use super::principal::{decode_principal, PrincipalUpsertRow, VerifiedIdentityProjection};
use super::store::{PostgresProductIdentityStore, SECRET_INSERT_ATTEMPTS};
use super::{ConsumedOAuthFlowV1, IssuedProductSessionV1, ProductIdentityError};

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
                    let Some(principal_id) = row.principal_id else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    let Some(discord_user_id) = row.discord_user_id else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    let Some(identity_revision) = row.identity_revision else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    let Some(display_profile) = row.display_profile else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    let Some(idle_expires_at) = row.idle_expires_at else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    let Some(absolute_expires_at) = row.absolute_expires_at else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    let Some(database_now) = row.database_now else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    let principal = match decode_principal(
                        PrincipalUpsertRow {
                            principal_id,
                            discord_user_id,
                            identity_revision,
                            display_profile,
                        },
                        session_digest,
                        absolute_expires_at,
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
                    if principal.discord_user_id() != identity.discord_user_id
                        || (!exact_replay && principal.display_name() != identity.display_name)
                        || idle_expires_at > absolute_expires_at
                    {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    }
                    let Some(max_age_seconds) = remaining_seconds(database_now, idle_expires_at)
                    else {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    };
                    if max_age_seconds > 1_800 {
                        transaction
                            .rollback()
                            .await
                            .map_err(identity_database_error)?;
                        return Err(ProductIdentityError::Invariant);
                    }
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
                "digest_conflict" => {
                    let valid_shape =
                        row.failure_projection_is_empty() && row.database_now.is_some();
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    if !valid_shape {
                        return Err(ProductIdentityError::Invariant);
                    }
                }
                "flow_invalid_or_consumed" => {
                    let valid_shape = row.failure_projection_is_empty();
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    return if valid_shape {
                        Err(ProductIdentityError::FlowInvalidOrConsumed)
                    } else {
                        Err(ProductIdentityError::Invariant)
                    };
                }
                "principal_disabled" => {
                    let valid_shape = row.failure_projection_is_empty();
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    return if valid_shape {
                        Err(ProductIdentityError::PrincipalDisabled)
                    } else {
                        Err(ProductIdentityError::Invariant)
                    };
                }
                _ => {
                    transaction
                        .rollback()
                        .await
                        .map_err(identity_database_error)?;
                    return Err(ProductIdentityError::Invariant);
                }
            }
        }
        Err(ProductIdentityError::SecretGeneration)
    }
}
