use std::num::NonZeroU64;

use authoring_application_discord::VerifiedDiscordIdentityV1;
use authoring_promotion::PrincipalId;
use chrono::{DateTime, Utc};
use discord_model::UserId;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;

use crate::authentication::{load_active_product_session, ActiveProductSessionV1};
use crate::{ProductSecretGenerator, ProductSessionDigestV1};

use super::database::map_session_validation;
use super::store::PostgresProductIdentityStore;
use super::{CurrentProductPrincipalV1, ProductIdentityError};

const DISPLAY_NAME_MAX_BYTES: usize = 512;
const DISPLAY_NAME_MAX_SCALARS: usize = 128;

#[derive(Clone, Copy)]
pub(super) struct VerifiedIdentityProjection<'a> {
    pub(super) discord_user_id: UserId,
    pub(super) display_name: &'a str,
}

impl<'a> VerifiedIdentityProjection<'a> {
    pub(super) fn from_capability(identity: &'a VerifiedDiscordIdentityV1) -> Self {
        Self {
            discord_user_id: identity.user_id(),
            display_name: identity.display_name(),
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct PrincipalUpsertRow {
    pub(super) principal_id: String,
    pub(super) discord_user_id: String,
    pub(super) identity_revision: i64,
    pub(super) display_profile: Json<serde_json::Value>,
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
    pub async fn current_principal(
        &self,
        credential: &str,
    ) -> Result<CurrentProductPrincipalV1, ProductIdentityError> {
        let active = load_active_product_session(
            &self.pools.session_api,
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
            &self.pools.session_api,
            self.config.lifetimes().authentication(),
            credential,
            Some(csrf),
        )
        .await
        .map_err(map_session_validation)?;
        decode_active_principal(active)
    }
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

pub(super) fn decode_principal(
    row: PrincipalUpsertRow,
    session_fingerprint: ProductSessionDigestV1,
    absolute_expires_at: DateTime<Utc>,
    invalid: ProductIdentityError,
) -> Result<CurrentProductPrincipalV1, ProductIdentityError> {
    let principal_id = PrincipalId::parse(&row.principal_id).map_err(|_| invalid)?;
    let discord_user_id = canonical_snowflake(&row.discord_user_id)
        .map(UserId)
        .ok_or(invalid)?;
    if principal_id.as_str() != format!("discord:{discord_user_id}") {
        return Err(invalid);
    }
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
