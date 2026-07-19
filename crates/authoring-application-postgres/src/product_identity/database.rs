use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::{Postgres, Transaction};

use crate::authentication::SessionValidationError;
use crate::ProductDatabaseFailureV1;

use super::{OAuthFlowError, ProductIdentityError};

pub(super) async fn begin_bounded_identity_transaction<'a>(
    pool: &'a PgPool,
    timeout: &str,
) -> Result<Transaction<'a, Postgres>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ WRITE")
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "SELECT pg_catalog.set_config('statement_timeout', $1, true), \
         pg_catalog.set_config('lock_timeout', $1, true), \
         pg_catalog.set_config('idle_in_transaction_session_timeout', $1, true)",
    )
    .bind(timeout)
    .execute(&mut *transaction)
    .await?;
    Ok(transaction)
}

pub(super) fn map_session_validation(error: SessionValidationError) -> ProductIdentityError {
    match error {
        SessionValidationError::InvalidCredential => ProductIdentityError::InvalidCredential,
        SessionValidationError::InvalidCsrf => ProductIdentityError::InvalidCsrf,
        SessionValidationError::Expired => ProductIdentityError::Expired,
        SessionValidationError::Revoked => ProductIdentityError::Revoked,
        SessionValidationError::Database(error) => ProductIdentityError::Database(error),
        SessionValidationError::Invariant => ProductIdentityError::Invariant,
    }
}

pub(super) fn oauth_database_error(error: sqlx::Error) -> OAuthFlowError {
    ProductDatabaseFailureV1::classify(&error).into()
}

pub(super) fn identity_database_error(error: sqlx::Error) -> ProductIdentityError {
    ProductDatabaseFailureV1::classify(&error).into()
}

pub(super) fn remaining_seconds(now: DateTime<Utc>, expires_at: DateTime<Utc>) -> Option<u32> {
    let seconds = (expires_at - now).num_seconds();
    u32::try_from(seconds).ok().filter(|seconds| *seconds != 0)
}
