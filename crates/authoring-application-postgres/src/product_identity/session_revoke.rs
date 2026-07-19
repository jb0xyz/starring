use chrono::{DateTime, Utc};
use subtle::ConstantTimeEq;

use crate::digest::{csrf_comparison_tag_v1, digest_opaque_session_credential_v1};
use crate::ProductSecretGenerator;

use super::database::{begin_bounded_identity_transaction, identity_database_error};
use super::store::PostgresProductIdentityStore;
use super::{ProductIdentityError, ProductLogoutDispositionV1, ProductSessionRevocationReasonV1};

const LOGOUT_READ_QUERY: &str = "SELECT * FROM public.starring_product_session_logout_read_v1($1)";
const LOGOUT_COMMIT_QUERY: &str =
    "SELECT public.starring_product_session_logout_commit_v1($1, $2, $3)";
const SECURITY_REVOKE_QUERY: &str =
    "SELECT * FROM public.starring_product_session_security_revoke_v1($1)";

#[derive(sqlx::FromRow)]
struct LogoutSessionRow {
    csrf_digest_length: i32,
    oauth_state_digest_length: Option<i32>,
    csrf_comparison_tag: Vec<u8>,
    last_seen_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    revocation_reason: Option<String>,
}

#[derive(sqlx::FromRow)]
struct SecurityRevokeRow {
    outcome_code: String,
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
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let mut transaction =
            begin_bounded_identity_transaction(&self.pools.session_api, timeout.as_str())
                .await
                .map_err(identity_database_error)?;
        let row = sqlx::query_as::<_, LogoutSessionRow>(LOGOUT_READ_QUERY)
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
        let persisted_tag: [u8; 32] = match row.csrf_comparison_tag.as_slice().try_into() {
            Ok(tag) if row.csrf_digest_length == 32 => tag,
            _ => {
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                return Err(ProductIdentityError::Invariant);
            }
        };
        let expected_tag =
            csrf_comparison_tag_v1(session_digest.as_bytes(), csrf_digest.as_bytes());
        if persisted_tag.ct_eq(&expected_tag).unwrap_u8() != 1 {
            transaction
                .rollback()
                .await
                .map_err(identity_database_error)?;
            return Err(ProductIdentityError::InvalidCsrf);
        }
        if row.revoked_at.is_some() {
            let outcome = if row.revocation_reason.as_deref() == Some("user_logout") {
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
        if row.revocation_reason.is_some() || row.oauth_state_digest_length != Some(32) {
            transaction
                .rollback()
                .await
                .map_err(identity_database_error)?;
            return Err(ProductIdentityError::Invariant);
        }
        let revoked = sqlx::query_scalar::<_, i64>(LOGOUT_COMMIT_QUERY)
            .bind(session_digest.as_bytes().as_slice())
            .bind(persisted_tag.as_slice())
            .bind(row.last_seen_at)
            .fetch_one(&mut *transaction)
            .await
            .map_err(identity_database_error)?;
        if revoked != 1 {
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

    pub async fn revoke_session(
        &self,
        credential: &str,
        reason: ProductSessionRevocationReasonV1,
    ) -> Result<ProductLogoutDispositionV1, ProductIdentityError> {
        if reason != ProductSessionRevocationReasonV1::SecurityRevocation {
            return Err(ProductIdentityError::Invariant);
        }
        let session_digest = digest_opaque_session_credential_v1(credential)
            .map_err(|_| ProductIdentityError::InvalidCredential)?;
        let timeout = self.config.lifetimes().authentication().statement_timeout();
        let mut transaction =
            begin_bounded_identity_transaction(&self.pools.security_revoker, timeout.as_str())
                .await
                .map_err(identity_database_error)?;
        let row = sqlx::query_as::<_, SecurityRevokeRow>(SECURITY_REVOKE_QUERY)
            .bind(session_digest.as_bytes().as_slice())
            .fetch_one(&mut *transaction)
            .await
            .map_err(identity_database_error)?;
        match row.outcome_code.as_str() {
            "revoked" => {
                transaction
                    .commit()
                    .await
                    .map_err(|_| ProductIdentityError::CommitIndeterminate)?;
                Ok(ProductLogoutDispositionV1::Revoked)
            }
            "exact_replay" => {
                transaction
                    .commit()
                    .await
                    .map_err(identity_database_error)?;
                Ok(ProductLogoutDispositionV1::ExactReplay)
            }
            "already_revoked" => {
                transaction
                    .commit()
                    .await
                    .map_err(identity_database_error)?;
                Err(ProductIdentityError::Revoked)
            }
            "invalid_credential" => {
                transaction
                    .rollback()
                    .await
                    .map_err(identity_database_error)?;
                Err(ProductIdentityError::InvalidCredential)
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
}
