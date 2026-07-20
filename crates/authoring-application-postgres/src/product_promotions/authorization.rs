use std::fmt::{Debug, Formatter};

use authoring_application::{
    AuthorizedPromotionAccessV1, AuthorizedPromotionSubmissionErrorV1,
    AuthorizedPromotionSubmissionV1, CapabilityV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use chrono::{DateTime, Duration, Utc};
use discord_model::Permissions;

const MAX_PROMOTE_AUTHORITY_LIFETIME: Duration = Duration::seconds(5);

pub(super) struct ProductPromotionAccessArgsV1 {
    pub expected_tenant_id: String,
    pub expected_installation_id: String,
    pub expected_principal_id: String,
    pub expected_product_session_digest: Vec<u8>,
    pub expected_acting_user_id: String,
    pub expected_discord_application_id: String,
    pub expected_guild_id: String,
    pub expected_capability: String,
    pub observed_current_authority_revision: i64,
    pub observed_current_authority_payload_digest: String,
    pub authority_observation_digest: String,
    pub authority_observed_at: DateTime<Utc>,
    pub authority_expires_at: DateTime<Utc>,
    pub effective_permission_bits: String,
    pub guild_owner: bool,
}

impl Debug for ProductPromotionAccessArgsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductPromotionAccessArgsV1(<redacted>)")
    }
}

pub(super) fn product_promotion_access_args_v1(
    access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<ProductPromotionAccessArgsV1, AuthorizedPromotionSubmissionErrorV1> {
    let scope = access.scope();
    let evidence = access.evidence();
    let source = AuthorityScalarSourceV1 {
        tenant_matches: evidence.tenant_id() == scope.tenant_id(),
        installation_matches: evidence.installation_id() == scope.installation_id(),
        guild_matches: evidence.guild_id() == scope.guild_id(),
        acting_user_matches: evidence.acting_user_id() == scope.acting_user_id(),
        capability: evidence.capability(),
        effective_permission_bits: evidence.effective_permissions().bits(),
        guild_owner: evidence.owner(),
        authority_revision: evidence.installation_authority_revision().get(),
        authority_payload_digest: evidence.installation_authority_digest(),
        observation_digest: evidence.observation_digest(),
        observed_at: evidence.observed_at(),
        expires_at: evidence.expires_at(),
    };
    validate_authority_source_v1(&source)?;
    let observed_current_authority_revision = i64::try_from(source.authority_revision)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::Forbidden)?;
    let authority_observed_at = postgres_timestamp_v1(source.observed_at)?;
    let authority_expires_at = postgres_timestamp_v1(source.expires_at)?;
    if authority_observed_at >= authority_expires_at {
        return Err(AuthorizedPromotionSubmissionErrorV1::Forbidden);
    }
    Ok(ProductPromotionAccessArgsV1 {
        expected_tenant_id: scope.tenant_id().as_str().to_string(),
        expected_installation_id: scope.installation_id().as_str().to_string(),
        expected_principal_id: access.actor().principal_id().as_str().to_string(),
        expected_product_session_digest: access.session_fingerprint().as_bytes().to_vec(),
        expected_acting_user_id: scope.acting_user_id().to_string(),
        expected_discord_application_id: evidence.application_id().get().to_string(),
        expected_guild_id: scope.guild_id().to_string(),
        expected_capability: "promote".to_string(),
        observed_current_authority_revision,
        observed_current_authority_payload_digest: source.authority_payload_digest.to_string(),
        authority_observation_digest: source.observation_digest.to_string(),
        authority_observed_at,
        authority_expires_at,
        effective_permission_bits: source.effective_permission_bits.to_string(),
        guild_owner: source.guild_owner,
    })
}

fn postgres_timestamp_v1(
    value: DateTime<Utc>,
) -> Result<DateTime<Utc>, AuthorizedPromotionSubmissionErrorV1> {
    DateTime::from_timestamp_micros(value.timestamp_micros())
        .ok_or(AuthorizedPromotionSubmissionErrorV1::Forbidden)
}

pub(super) fn validate_product_promotion_submission_v1(
    request: &AuthorizedPromotionSubmissionV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> Result<ProductPromotionAccessArgsV1, AuthorizedPromotionSubmissionErrorV1> {
    let access = request.access();
    let context = &request.input().context;
    if context.tenant_id != *access.scope().tenant_id()
        || context.installation_id != *access.scope().installation_id()
        || context.guild_id != access.scope().guild_id()
        || context.requester != access.scope().acting_user_id()
        || context.principal_id != *access.actor().principal_id()
        || context.session_owner_id != *access.actor().principal_id()
        || context.session_id != *access.session_id()
        || context.session_generation != access.expected_generation()
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch);
    }
    product_promotion_access_args_v1(access)
}

struct AuthorityScalarSourceV1<'a> {
    tenant_matches: bool,
    installation_matches: bool,
    guild_matches: bool,
    acting_user_matches: bool,
    capability: CapabilityV1,
    effective_permission_bits: u64,
    guild_owner: bool,
    authority_revision: u64,
    authority_payload_digest: &'a str,
    observation_digest: &'a str,
    observed_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

fn validate_authority_source_v1(
    source: &AuthorityScalarSourceV1<'_>,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if !source.tenant_matches
        || !source.installation_matches
        || !source.guild_matches
        || !source.acting_user_matches
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch);
    }
    if source.capability != CapabilityV1::Promote
        || source.authority_revision == 0
        || source.authority_revision > i64::MAX as u64
        || !is_lower_hex_digest(source.authority_payload_digest)
        || !is_lower_hex_digest(source.observation_digest)
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::Forbidden);
    }
    let permissions = Permissions::from_bits_retain(source.effective_permission_bits);
    if !source.guild_owner
        && !permissions.intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD)
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::Forbidden);
    }
    let latest_expiry = source
        .observed_at
        .checked_add_signed(MAX_PROMOTE_AUTHORITY_LIFETIME)
        .ok_or(AuthorizedPromotionSubmissionErrorV1::Forbidden)?;
    if source.expires_at <= source.observed_at || source.expires_at > latest_expiry {
        return Err(AuthorizedPromotionSubmissionErrorV1::Forbidden);
    }
    Ok(())
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> AuthorityScalarSourceV1<'static> {
        let observed_at = DateTime::parse_from_rfc3339("2026-07-20T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        AuthorityScalarSourceV1 {
            tenant_matches: true,
            installation_matches: true,
            guild_matches: true,
            acting_user_matches: true,
            capability: CapabilityV1::Promote,
            effective_permission_bits: Permissions::MANAGE_GUILD.bits(),
            guild_owner: false,
            authority_revision: 3,
            authority_payload_digest:
                "ab11ab11ab11ab11ab11ab11ab11ab11ab11ab11ab11ab11ab11ab11ab11ab11",
            observation_digest: "cd22cd22cd22cd22cd22cd22cd22cd22cd22cd22cd22cd22cd22cd22cd22cd22",
            observed_at,
            expires_at: observed_at + Duration::seconds(5),
        }
    }

    #[test]
    fn promote_authority_requires_exact_scope_capability_and_bounded_evidence() {
        assert_eq!(validate_authority_source_v1(&source()), Ok(()));
        let mut wrong_scope = source();
        wrong_scope.guild_matches = false;
        assert_eq!(
            validate_authority_source_v1(&wrong_scope),
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        );
        let mut wrong_capability = source();
        wrong_capability.capability = CapabilityV1::Approve;
        assert_eq!(
            validate_authority_source_v1(&wrong_capability),
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        );
        let mut long_lived = source();
        long_lived.expires_at = long_lived.observed_at + Duration::seconds(6);
        assert_eq!(
            validate_authority_source_v1(&long_lived),
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        );
    }

    #[test]
    fn promote_authority_requires_owner_or_management_permission() {
        let mut unprivileged = source();
        unprivileged.effective_permission_bits = 0;
        assert_eq!(
            validate_authority_source_v1(&unprivileged),
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        );
        unprivileged.guild_owner = true;
        assert_eq!(validate_authority_source_v1(&unprivileged), Ok(()));
    }

    #[test]
    fn promote_authority_rejects_malformed_digests_and_revision_overflow() {
        let mut malformed = source();
        malformed.observation_digest = "ABC";
        assert_eq!(
            validate_authority_source_v1(&malformed),
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        );
        let mut overflow = source();
        overflow.authority_revision = i64::MAX as u64 + 1;
        assert_eq!(
            validate_authority_source_v1(&overflow),
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        );
    }

    #[test]
    fn authority_timestamps_are_canonicalized_to_postgres_precision() {
        let base = source().observed_at;
        let canonical = postgres_timestamp_v1(base + Duration::nanoseconds(999)).unwrap();
        assert_eq!(canonical, base);
        assert_eq!(canonical.timestamp_subsec_nanos() % 1_000, 0);
    }
}
