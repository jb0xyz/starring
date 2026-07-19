use chrono::{DateTime, Utc};
use subtle::ConstantTimeEq;

use crate::digest::digest_opaque_session_credential_v1;
use crate::{ProductSecretGenerator, ProductSessionDigestV1};

use super::database::{database_time, identity_database_error, set_statement_timeout};
use super::store::PostgresProductIdentityStore;
use super::{ProductIdentityError, ProductLogoutDispositionV1, ProductSessionRevocationReasonV1};

#[derive(sqlx::FromRow)]
struct LogoutSessionRow {
    csrf_digest: Vec<u8>,
    oauth_state_digest: Option<Vec<u8>>,
    revoked_at: Option<DateTime<Utc>>,
    revocation_reason: Option<String>,
}

impl<G> PostgresProductIdentityStore<G>
where
    G: ProductSecretGenerator,
{
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
}
