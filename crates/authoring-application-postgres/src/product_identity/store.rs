use std::num::NonZeroU64;

use authoring_application_discord::VerifiedDiscordIdentityV1;
use authoring_promotion::PrincipalId;
use chrono::{DateTime, Utc};
use discord_model::UserId;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgPool;
use sqlx::types::Json;
use subtle::ConstantTimeEq;

use crate::authentication::{
    load_active_product_session, ActiveProductSessionV1, SessionValidationError,
};
use crate::digest::digest_opaque_session_credential_v1;
use crate::{
    OperatingSystemSecretGenerator, PostgresAuthentication, ProductDatabaseFailureV1,
    ProductSecretGenerator, ProductSecretGeneratorError, ProductSecretV1, ProductSessionDigestV1,
};

use super::{
    ConsumedOAuthFlowV1, CurrentProductPrincipalV1, IssuedProductSessionV1, OAuthFlowError,
    OAuthFlowIssueV1, PostgresProductIdentityConfig, ProductIdentityError,
    ProductLogoutDispositionV1, ProductSessionRevocationReasonV1,
};

const SECRET_INSERT_ATTEMPTS: usize = 4;
const DISPLAY_NAME_MAX_BYTES: usize = 512;
const DISPLAY_NAME_MAX_SCALARS: usize = 128;

#[derive(Clone, Copy)]
struct VerifiedIdentityProjection<'a> {
    discord_user_id: UserId,
    display_name: &'a str,
}

impl<'a> VerifiedIdentityProjection<'a> {
    fn from_capability(identity: &'a VerifiedDiscordIdentityV1) -> Self {
        Self {
            discord_user_id: identity.user_id(),
            display_name: identity.display_name(),
        }
    }
}

#[derive(Clone)]
pub struct PostgresProductIdentityStore<G = OperatingSystemSecretGenerator> {
    pool: PgPool,
    generator: G,
    config: PostgresProductIdentityConfig,
}

impl PostgresProductIdentityStore<OperatingSystemSecretGenerator> {
    pub fn production(pool: PgPool, config: PostgresProductIdentityConfig) -> Self {
        Self::new(pool, OperatingSystemSecretGenerator, config)
    }
}

impl<G> PostgresProductIdentityStore<G> {
    pub fn new(pool: PgPool, generator: G, config: PostgresProductIdentityConfig) -> Self {
        Self {
            pool,
            generator,
            config,
        }
    }

    pub fn authentication(&self) -> PostgresAuthentication {
        PostgresAuthentication::with_config(
            self.pool.clone(),
            self.config.lifetimes().authentication(),
        )
    }
}

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

#[derive(sqlx::FromRow)]
struct ConsumedOAuthFlowValidationRow {
    redirect_uri: String,
    return_path: String,
    consumed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct PrincipalUpsertRow {
    principal_id: String,
    discord_user_id: String,
    identity_revision: i64,
    display_profile: Json<serde_json::Value>,
}

#[derive(sqlx::FromRow)]
struct SessionInsertRow {
    idle_expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct LogoutSessionRow {
    csrf_digest: Vec<u8>,
    oauth_state_digest: Option<Vec<u8>>,
    revoked_at: Option<DateTime<Utc>>,
    revocation_reason: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDisplayProfileV1 {
    display_name: String,
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

    pub async fn issue_product_session(
        &self,
        consumed_flow: ConsumedOAuthFlowV1,
        identity: VerifiedDiscordIdentityV1,
    ) -> Result<IssuedProductSessionV1, ProductIdentityError> {
        let identity = VerifiedIdentityProjection::from_capability(&identity);
        self.issue_product_session_core(consumed_flow, identity)
            .await
    }

    async fn issue_product_session_core(
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

    pub async fn current_principal(
        &self,
        credential: &str,
    ) -> Result<CurrentProductPrincipalV1, ProductIdentityError> {
        let active = load_active_product_session(
            &self.pool,
            self.config.lifetimes().authentication(),
            credential,
            None,
        )
        .await
        .map_err(map_session_validation)?;
        decode_active_principal(active)
    }

    pub async fn verify_csrf(
        &self,
        credential: &str,
        csrf: &str,
    ) -> Result<CurrentProductPrincipalV1, ProductIdentityError> {
        let active = load_active_product_session(
            &self.pool,
            self.config.lifetimes().authentication(),
            credential,
            Some(csrf),
        )
        .await
        .map_err(map_session_validation)?;
        decode_active_principal(active)
    }

    pub async fn logout(
        &self,
        credential: &str,
        csrf: &str,
    ) -> Result<ProductLogoutDispositionV1, ProductIdentityError> {
        let session_digest = digest_opaque_session_credential_v1(credential)
            .map_err(|_| ProductIdentityError::InvalidCredential)?;
        let csrf_digest = digest_opaque_session_credential_v1(csrf)
            .map_err(|_| ProductIdentityError::InvalidCsrf)?;
        if session_digest == csrf_digest {
            return Err(ProductIdentityError::InvalidCsrf);
        }
        self.revoke_locked_session(
            session_digest,
            Some(csrf_digest),
            ProductSessionRevocationReasonV1::UserLogout,
        )
        .await
    }

    pub async fn revoke_session(
        &self,
        credential: &str,
        reason: ProductSessionRevocationReasonV1,
    ) -> Result<ProductLogoutDispositionV1, ProductIdentityError> {
        let session_digest = digest_opaque_session_credential_v1(credential)
            .map_err(|_| ProductIdentityError::InvalidCredential)?;
        self.revoke_locked_session(session_digest, None, reason)
            .await
    }

    async fn revoke_locked_session(
        &self,
        session_digest: ProductSessionDigestV1,
        expected_csrf_digest: Option<ProductSessionDigestV1>,
        reason: ProductSessionRevocationReasonV1,
    ) -> Result<ProductLogoutDispositionV1, ProductIdentityError> {
        let mut transaction = self.pool.begin().await.map_err(identity_database_error)?;
        set_statement_timeout(
            &mut transaction,
            self.config.lifetimes().authentication().statement_timeout(),
        )
        .await
        .map_err(identity_database_error)?;
        let row = sqlx::query_as::<_, LogoutSessionRow>(
            "SELECT csrf_digest, oauth_state_digest, revoked_at, revocation_reason \
             FROM public.product_auth_sessions WHERE session_digest = $1 FOR UPDATE",
        )
        .bind(session_digest.as_bytes().as_slice())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(identity_database_error)?;
        let Some(row) = row else {
            transaction
                .rollback()
                .await
                .map_err(identity_database_error)?;
            return Err(ProductIdentityError::InvalidCredential);
        };
        if let Some(expected) = expected_csrf_digest {
            let Ok(persisted): Result<[u8; 32], _> = row.csrf_digest.as_slice().try_into() else {
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(ProductIdentityError::Invariant);
            };
            if persisted.ct_eq(expected.as_bytes()).unwrap_u8() != 1 {
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(ProductIdentityError::InvalidCsrf);
            }
        }
        if row.revoked_at.is_some() {
            let outcome = if row.revocation_reason.as_deref() == Some(reason.as_str()) {
                Ok(ProductLogoutDispositionV1::ExactReplay)
            } else {
                Err(ProductIdentityError::Revoked)
            };
            transaction
                .commit()
                .await
                .map_err(identity_database_error)?;
            return outcome;
        }
        if row.oauth_state_digest.as_deref().map(<[u8]>::len) != Some(32) {
            transaction
                .rollback()
                .await
                .map_err(identity_database_error)?;
            return Err(ProductIdentityError::Invariant);
        }
        let database_now = match database_time(&mut transaction).await {
            Ok(database_now) => database_now,
            Err(error) => {
                let failure = identity_database_error(error);
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(failure);
            }
        };
        let result = sqlx::query(
            "UPDATE public.product_auth_sessions \
             SET revoked_at = GREATEST($2, last_seen_at), \
             revocation_reason = $3 \
             WHERE session_digest = $1 AND revoked_at IS NULL",
        )
        .bind(session_digest.as_bytes().as_slice())
        .bind(database_now)
        .bind(reason.as_str())
        .execute(&mut *transaction)
        .await;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                let failure = identity_database_error(error);
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(failure);
            }
        };
        if result.rows_affected() != 1 {
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
        Ok(ProductLogoutDispositionV1::Revoked)
    }

    fn generate_distinct_pair(
        &self,
    ) -> Result<(ProductSecretV1, ProductSecretV1), ProductSecretGeneratorError> {
        for _ in 0..SECRET_INSERT_ATTEMPTS {
            let first = ProductSecretV1::generate(&self.generator)?;
            let second = ProductSecretV1::generate(&self.generator)?;
            if first.expose_secret() != second.expose_secret() {
                return Ok((first, second));
            }
        }
        Err(ProductSecretGeneratorError::Unavailable)
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

async fn upsert_principal(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: VerifiedIdentityProjection<'_>,
) -> Result<PrincipalUpsertRow, ProductIdentityError> {
    let canonical_principal_id =
        PrincipalId::parse(&format!("discord:{}", identity.discord_user_id))
            .map_err(|_| ProductIdentityError::Invariant)?;
    let display_profile = json!({"display_name": identity.display_name});
    sqlx::query_as::<_, PrincipalUpsertRow>(
        "WITH principal_clock AS MATERIALIZED ( \
          SELECT pg_catalog.clock_timestamp() AS authenticated_at \
         ) \
         INSERT INTO public.product_principals AS principal_record \
         (principal_id, discord_user_id, display_profile, last_authenticated_at, updated_at) \
         SELECT $1, $2, $3, authenticated_at, authenticated_at FROM principal_clock \
         ON CONFLICT (discord_user_id) DO UPDATE SET \
         identity_revision = principal_record.identity_revision + 1, \
         display_profile = EXCLUDED.display_profile, \
         last_authenticated_at = GREATEST( \
          EXCLUDED.last_authenticated_at, principal_record.updated_at + INTERVAL '1 microsecond'), \
         updated_at = GREATEST( \
          EXCLUDED.updated_at, principal_record.updated_at + INTERVAL '1 microsecond') \
         WHERE NOT principal_record.disabled \
         RETURNING principal_id, discord_user_id, identity_revision, display_profile",
    )
    .bind(canonical_principal_id.as_str())
    .bind(identity.discord_user_id.to_string())
    .bind(Json(display_profile))
    .fetch_optional(&mut **transaction)
    .await
    .map_err(identity_database_error)?
    .ok_or(ProductIdentityError::PrincipalDisabled)
}

fn decode_active_principal(
    active: ActiveProductSessionV1,
) -> Result<CurrentProductPrincipalV1, ProductIdentityError> {
    let row = PrincipalUpsertRow {
        principal_id: active.principal_id.to_string(),
        discord_user_id: active.discord_user_id,
        identity_revision: i64::try_from(active.identity_revision)
            .map_err(|_| ProductIdentityError::Invariant)?,
        display_profile: Json(active.display_profile),
    };
    decode_principal(
        row,
        active.session_fingerprint,
        active.absolute_expires_at,
        ProductIdentityError::Invariant,
    )
}

fn decode_principal(
    row: PrincipalUpsertRow,
    session_fingerprint: ProductSessionDigestV1,
    absolute_expires_at: DateTime<Utc>,
    invalid: ProductIdentityError,
) -> Result<CurrentProductPrincipalV1, ProductIdentityError> {
    let principal_id = PrincipalId::parse(&row.principal_id).map_err(|_| invalid)?;
    let discord_user_id = canonical_snowflake(&row.discord_user_id)
        .map(UserId)
        .ok_or(invalid)?;
    let identity_revision = u64::try_from(row.identity_revision)
        .ok()
        .and_then(NonZeroU64::new)
        .map(NonZeroU64::get)
        .ok_or(invalid)?;
    let display_profile = serde_json::from_value::<StoredDisplayProfileV1>(row.display_profile.0)
        .map_err(|_| invalid)?;
    if !valid_stored_display_name(&display_profile.display_name) {
        return Err(invalid);
    }
    Ok(CurrentProductPrincipalV1::from_authenticated_session(
        principal_id,
        session_fingerprint,
        discord_user_id,
        display_profile.display_name,
        identity_revision,
        absolute_expires_at,
    ))
}

async fn set_statement_timeout(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    timeout: String,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(timeout)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn database_time(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut **transaction)
        .await
}

fn map_session_validation(error: SessionValidationError) -> ProductIdentityError {
    match error {
        SessionValidationError::InvalidCredential => ProductIdentityError::InvalidCredential,
        SessionValidationError::InvalidCsrf => ProductIdentityError::InvalidCsrf,
        SessionValidationError::Expired => ProductIdentityError::Expired,
        SessionValidationError::Revoked => ProductIdentityError::Revoked,
        SessionValidationError::Database(error) => ProductIdentityError::Database(error),
        SessionValidationError::Invariant => ProductIdentityError::Invariant,
    }
}

fn oauth_database_error(error: sqlx::Error) -> OAuthFlowError {
    ProductDatabaseFailureV1::classify(&error).into()
}

fn identity_database_error(error: sqlx::Error) -> ProductIdentityError {
    ProductDatabaseFailureV1::classify(&error).into()
}

fn remaining_seconds(now: DateTime<Utc>, expires_at: DateTime<Utc>) -> Option<u32> {
    let seconds = (expires_at - now).num_seconds();
    u32::try_from(seconds).ok().filter(|seconds| *seconds != 0)
}

fn unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505")
    )
}

fn constraint_name(error: &sqlx::Error) -> Option<&str> {
    match error {
        sqlx::Error::Database(database) => database.constraint(),
        _ => None,
    }
}

fn oauth_flow_constraint(error: &sqlx::Error) -> bool {
    matches!(
        constraint_name(error),
        Some(
            "product_auth_sessions_oauth_state_unique"
                | "product_auth_sessions_oauth_state_fk"
                | "product_auth_sessions_oauth_binding_valid"
        )
    )
}

fn canonical_snowflake(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed != 0 && parsed.to_string() == value).then_some(parsed)
}

fn valid_stored_display_name(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && value.len() <= DISPLAY_NAME_MAX_BYTES
        && value.chars().count() <= DISPLAY_NAME_MAX_SCALARS
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use chrono::TimeDelta;
    use futures::join;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

    use super::*;
    use crate::{ProductIdentityLifetimesV1, ProductSecretGeneratorError, MIGRATOR};

    #[derive(Clone)]
    struct DeterministicGenerator {
        counter: Arc<AtomicU64>,
    }

    impl ProductSecretGenerator for DeterministicGenerator {
        fn fill_secret(
            &self,
            destination: &mut [u8; 32],
        ) -> Result<(), ProductSecretGeneratorError> {
            let value = self.counter.fetch_add(1, Ordering::SeqCst);
            for (index, chunk) in destination.chunks_exact_mut(8).enumerate() {
                chunk.copy_from_slice(
                    &value
                        .wrapping_add(u64::try_from(index).unwrap())
                        .to_be_bytes(),
                );
            }
            Ok(())
        }
    }

    fn assert_test_database_name(database_name: &str) {
        assert!(
            database_name.starts_with("starring_")
                && database_name.split('_').any(|segment| segment == "test")
                && database_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        );
    }

    fn database_url() -> String {
        let url = std::env::var("STARRING_TEST_DATABASE_URL")
            .expect("STARRING_TEST_DATABASE_URL required for ignored PostgreSQL tests");
        let options = url
            .parse::<PgConnectOptions>()
            .unwrap_or_else(|_| panic!("STARRING_TEST_DATABASE_URL must be a PostgreSQL URL"));
        let database_name = options
            .get_database()
            .unwrap_or_else(|| panic!("STARRING_TEST_DATABASE_URL must name a database"));
        assert_test_database_name(database_name);
        url
    }

    fn unique_user_id() -> UserId {
        let value = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        UserId(u64::try_from(value).unwrap())
    }

    async fn consumed_flow(
        store: &PostgresProductIdentityStore<DeterministicGenerator>,
    ) -> ConsumedOAuthFlowV1 {
        let flow = store.create_oauth_flow("/").await.unwrap();
        store
            .consume_oauth_flow(
                flow.state().expose_secret(),
                flow.browser_nonce().expose_secret(),
            )
            .await
            .unwrap()
    }

    fn copy_consumed_flow(flow: &ConsumedOAuthFlowV1) -> ConsumedOAuthFlowV1 {
        ConsumedOAuthFlowV1 {
            state_digest: flow.state_digest.clone(),
            redirect_uri: flow.redirect_uri.clone(),
            return_path: flow.return_path.clone(),
            consumed_at: flow.consumed_at,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn private_issuer_core_persists_only_an_opaque_verified_projection() {
        let database_url = database_url();
        let expected_database = database_url
            .parse::<PgConnectOptions>()
            .unwrap()
            .get_database()
            .unwrap()
            .to_string();
        let setup_pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&database_url)
            .await
            .unwrap();
        let current_database =
            sqlx::query_scalar::<_, String>("SELECT pg_catalog.current_database()")
                .fetch_one(&setup_pool)
                .await
                .unwrap();
        assert_test_database_name(&current_database);
        assert_eq!(current_database, expected_database);
        MIGRATOR.run(&setup_pool).await.unwrap();
        sqlx::query("CREATE SCHEMA IF NOT EXISTS authoring_identity_shadow")
            .execute(&setup_pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE OR REPLACE FUNCTION authoring_identity_shadow.clock_timestamp() \
             RETURNS TIMESTAMPTZ LANGUAGE SQL IMMUTABLE SET search_path = pg_catalog \
             AS 'SELECT ''2000-01-01T00:00:00Z''::TIMESTAMPTZ'",
        )
        .execute(&setup_pool)
        .await
        .unwrap();
        let shadow_pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET search_path = authoring_identity_shadow, pg_catalog")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await
            .unwrap();
        let config = PostgresProductIdentityConfig::new(
            "https://starring.example/oauth/discord/callback",
            ["/".to_string()],
            ProductIdentityLifetimesV1::new(
                Duration::from_secs(600),
                Duration::from_secs(60),
                Duration::from_secs(300),
                Duration::from_secs(1),
                Duration::from_millis(250),
            )
            .unwrap(),
        )
        .unwrap();
        let seed = unique_user_id().0;
        let store = PostgresProductIdentityStore::new(
            shadow_pool,
            DeterministicGenerator {
                counter: Arc::new(AtomicU64::new(seed)),
            },
            config,
        );
        let user_id = unique_user_id();
        let invalid_flow = consumed_flow(&store).await;
        let invalid_user_id = unique_user_id();
        let invalid_projection = VerifiedIdentityProjection {
            discord_user_id: invalid_user_id,
            display_name: "Invalid Causality",
        };
        let invalid_claim = ConsumedOAuthFlowV1 {
            state_digest: invalid_flow.state_digest,
            redirect_uri: invalid_flow.redirect_uri,
            return_path: invalid_flow.return_path,
            consumed_at: invalid_flow.consumed_at + TimeDelta::seconds(1),
        };
        assert!(matches!(
            store
                .issue_product_session_core(invalid_claim, invalid_projection)
                .await,
            Err(ProductIdentityError::FlowInvalidOrConsumed)
        ));
        let invalid_principal_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM public.product_principals WHERE discord_user_id = $1",
        )
        .bind(invalid_user_id.to_string())
        .fetch_one(&setup_pool)
        .await
        .unwrap();
        assert_eq!(invalid_principal_count, 0);
        let raced_flow = consumed_flow(&store).await;
        let raced_flow_copy = copy_consumed_flow(&raced_flow);
        let left_user_id = unique_user_id();
        let right_user_id = UserId(left_user_id.0 + 1);
        let (left, right) = join!(
            store.issue_product_session_core(
                raced_flow,
                VerifiedIdentityProjection {
                    discord_user_id: left_user_id,
                    display_name: "Race Left",
                },
            ),
            store.issue_product_session_core(
                raced_flow_copy,
                VerifiedIdentityProjection {
                    discord_user_id: right_user_id,
                    display_name: "Race Right",
                },
            )
        );
        assert!(matches!(
            (left, right),
            (Ok(_), Err(ProductIdentityError::FlowInvalidOrConsumed))
                | (Err(ProductIdentityError::FlowInvalidOrConsumed), Ok(_))
        ));
        let raced_principal_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM public.product_principals \
             WHERE discord_user_id = ANY($1::TEXT[])",
        )
        .bind([left_user_id.to_string(), right_user_id.to_string()])
        .fetch_one(&setup_pool)
        .await
        .unwrap();
        assert_eq!(raced_principal_count, 1);
        let first = store
            .issue_product_session_core(
                consumed_flow(&store).await,
                VerifiedIdentityProjection {
                    discord_user_id: user_id,
                    display_name: "First Name",
                },
            )
            .await
            .unwrap();
        let first_session = first.session().expose_secret().to_string();
        let first_csrf = first.csrf().expose_secret().to_string();
        assert_eq!(first.principal().discord_user_id(), user_id);
        assert_eq!(first.principal().identity_revision(), 1);
        assert!((1..=60).contains(&first.max_age_seconds()));
        assert!(!format!("{first:?}").contains("First Name"));
        assert!(!format!("{first:?}").contains(&user_id.to_string()));
        let second = store
            .issue_product_session_core(
                consumed_flow(&store).await,
                VerifiedIdentityProjection {
                    discord_user_id: user_id,
                    display_name: "Second Name",
                },
            )
            .await
            .unwrap();
        assert_eq!(second.principal().identity_revision(), 2);
        assert_eq!(second.principal().display_name(), "Second Name");
        let current = store.current_principal(&first_session).await.unwrap();
        assert_eq!(current.identity_revision(), 2);
        assert_eq!(current.display_name(), "Second Name");
        sqlx::query(
            "WITH principal_clock AS MATERIALIZED ( \
              SELECT pg_catalog.clock_timestamp() AS disabled_at \
             ) \
             UPDATE public.product_principals AS principal \
             SET disabled = TRUE, identity_revision = principal.identity_revision + 1, \
              last_authenticated_at = GREATEST( \
               disabled_at, principal.updated_at + INTERVAL '1 microsecond'), \
              updated_at = GREATEST( \
               disabled_at, principal.updated_at + INTERVAL '1 microsecond') \
             FROM principal_clock WHERE principal.discord_user_id = $1",
        )
        .bind(user_id.to_string())
        .execute(&setup_pool)
        .await
        .unwrap();
        let disabled_flow = consumed_flow(&store).await;
        let disabled_state_digest = disabled_flow.state_digest.clone();
        assert!(matches!(
            store
                .issue_product_session_core(
                    disabled_flow,
                    VerifiedIdentityProjection {
                        discord_user_id: user_id,
                        display_name: "Disabled Name",
                    },
                )
                .await,
            Err(ProductIdentityError::PrincipalDisabled)
        ));
        let disabled_session_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM public.product_auth_sessions WHERE oauth_state_digest = $1",
        )
        .bind(disabled_state_digest.as_bytes().as_slice())
        .fetch_one(&setup_pool)
        .await
        .unwrap();
        assert_eq!(disabled_session_count, 0);
        store.logout(&first_session, &first_csrf).await.unwrap();
    }
}
