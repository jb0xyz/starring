use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

use crate::authentication::SessionValidationError;
use crate::ProductDatabaseFailureV1;

use super::{OAuthFlowError, ProductIdentityError};

pub(super) async fn set_statement_timeout(
    transaction: &mut Transaction<'_, Postgres>,
    timeout: String,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, true)")
        .bind(timeout)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub(super) async fn database_time(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar("SELECT pg_catalog.clock_timestamp()")
        .fetch_one(&mut **transaction)
        .await
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

pub(super) fn unique_violation(error: &sqlx::Error) -> bool {
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

pub(super) fn oauth_flow_constraint(error: &sqlx::Error) -> bool {
    matches!(
        constraint_name(error),
        Some(
            "product_auth_sessions_oauth_state_unique"
                | "product_auth_sessions_oauth_state_fk"
                | "product_auth_sessions_oauth_binding_valid"
        )
    )
}
