use std::fmt::{Display, Formatter};
use std::num::{NonZeroU32, NonZeroU64};

use crate::digest::LengthFramedSha256;
use crate::ResourceBindingFingerprint;

const AUTHORITY_PAYLOAD_DOMAIN_V1: &[u8] = b"starring.installation-authority.payload.v1\0";
const AUTHORITY_REQUEST_DOMAIN_V1: &[u8] = b"starring.installation-authority.request.v1\0";
const MAX_DATABASE_REVISION: u64 = i64::MAX as u64;
const REQUIRED_APPROVALS: u32 = 1;
const MAX_ACTIVATION_TTL_SECONDS: u64 = 31_536_000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstallationAuthorityPayloadDigestV1(String);

impl InstallationAuthorityPayloadDigestV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, InstallationAuthorityIdentityErrorV1> {
        let value = value.into();
        if canonical_digest(&value) {
            Ok(Self(value))
        } else {
            Err(InstallationAuthorityIdentityErrorV1::Digest)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for InstallationAuthorityPayloadDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstallationAuthorityRequestDigestV1(String);

impl InstallationAuthorityRequestDigestV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, InstallationAuthorityIdentityErrorV1> {
        let value = value.into();
        if canonical_digest(&value) {
            Ok(Self(value))
        } else {
            Err(InstallationAuthorityIdentityErrorV1::Digest)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for InstallationAuthorityRequestDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationAuthorityScopeV1<'a> {
    tenant_id: &'a str,
    installation_id: &'a str,
}

impl<'a> InstallationAuthorityScopeV1<'a> {
    pub fn new(
        tenant_id: &'a str,
        installation_id: &'a str,
    ) -> Result<Self, InstallationAuthorityIdentityErrorV1> {
        if !bounded_identifier(tenant_id) || !bounded_identifier(installation_id) {
            return Err(InstallationAuthorityIdentityErrorV1::Identifier);
        }
        Ok(Self {
            tenant_id,
            installation_id,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstallationAuthorityPolicyV1 {
    policy_revision: NonZeroU64,
    required_approvals: NonZeroU32,
    activation_ttl_seconds: NonZeroU64,
}

impl InstallationAuthorityPolicyV1 {
    pub fn new(
        policy_revision: NonZeroU64,
        required_approvals: NonZeroU32,
        activation_ttl_seconds: NonZeroU64,
    ) -> Result<Self, InstallationAuthorityIdentityErrorV1> {
        if policy_revision.get() > MAX_DATABASE_REVISION {
            return Err(InstallationAuthorityIdentityErrorV1::Revision);
        }
        if required_approvals.get() != REQUIRED_APPROVALS
            || activation_ttl_seconds.get() > MAX_ACTIVATION_TTL_SECONDS
        {
            return Err(InstallationAuthorityIdentityErrorV1::Policy);
        }
        Ok(Self {
            policy_revision,
            required_approvals,
            activation_ttl_seconds,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationAuthorityPayloadIdentityV1<'a> {
    scope: InstallationAuthorityScopeV1<'a>,
    revision: NonZeroU64,
    binding_revision: NonZeroU64,
    binding_fingerprint: &'a ResourceBindingFingerprint,
    policy: InstallationAuthorityPolicyV1,
}

impl<'a> InstallationAuthorityPayloadIdentityV1<'a> {
    pub fn new(
        scope: InstallationAuthorityScopeV1<'a>,
        revision: NonZeroU64,
        binding_revision: NonZeroU64,
        binding_fingerprint: &'a ResourceBindingFingerprint,
        policy: InstallationAuthorityPolicyV1,
    ) -> Result<Self, InstallationAuthorityIdentityErrorV1> {
        if revision.get() > MAX_DATABASE_REVISION || binding_revision.get() > MAX_DATABASE_REVISION
        {
            return Err(InstallationAuthorityIdentityErrorV1::Revision);
        }
        Ok(Self {
            scope,
            revision,
            binding_revision,
            binding_fingerprint,
            policy,
        })
    }

    pub fn revision(&self) -> NonZeroU64 {
        self.revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationAuthorityRequestIdentityV1<'a> {
    expected_head_revision: NonZeroU64,
    predecessor_authority_payload_digest: &'a InstallationAuthorityPayloadDigestV1,
    payload: &'a InstallationAuthorityPayloadIdentityV1<'a>,
    created_by_principal_id: &'a str,
}

impl<'a> InstallationAuthorityRequestIdentityV1<'a> {
    pub fn new(
        expected_head_revision: NonZeroU64,
        predecessor_authority_payload_digest: &'a InstallationAuthorityPayloadDigestV1,
        payload: &'a InstallationAuthorityPayloadIdentityV1<'a>,
        created_by_principal_id: &'a str,
    ) -> Result<Self, InstallationAuthorityIdentityErrorV1> {
        if expected_head_revision.get() > MAX_DATABASE_REVISION
            || expected_head_revision
                .get()
                .checked_add(1)
                .filter(|successor| *successor == payload.revision().get())
                .is_none()
        {
            return Err(InstallationAuthorityIdentityErrorV1::Revision);
        }
        if !bounded_identifier(created_by_principal_id) {
            return Err(InstallationAuthorityIdentityErrorV1::Identifier);
        }
        Ok(Self {
            expected_head_revision,
            predecessor_authority_payload_digest,
            payload,
            created_by_principal_id,
        })
    }
}

pub fn installation_authority_payload_digest_v1(
    input: &InstallationAuthorityPayloadIdentityV1<'_>,
) -> InstallationAuthorityPayloadDigestV1 {
    let mut digest = LengthFramedSha256::new(AUTHORITY_PAYLOAD_DOMAIN_V1);
    digest.update(input.scope.tenant_id.as_bytes());
    digest.update(input.scope.installation_id.as_bytes());
    digest.update(&input.revision.get().to_be_bytes());
    digest.update(&input.binding_revision.get().to_be_bytes());
    digest.update(input.binding_fingerprint.as_str().as_bytes());
    digest.update(&input.policy.policy_revision.get().to_be_bytes());
    digest.update(&input.policy.required_approvals.get().to_be_bytes());
    digest.update(&input.policy.activation_ttl_seconds.get().to_be_bytes());
    InstallationAuthorityPayloadDigestV1(digest.finalize())
}

pub fn installation_authority_request_digest_v1(
    input: &InstallationAuthorityRequestIdentityV1<'_>,
) -> InstallationAuthorityRequestDigestV1 {
    let payload_digest = installation_authority_payload_digest_v1(input.payload);
    let mut digest = LengthFramedSha256::new(AUTHORITY_REQUEST_DOMAIN_V1);
    digest.update(input.payload.scope.tenant_id.as_bytes());
    digest.update(input.payload.scope.installation_id.as_bytes());
    digest.update(&input.expected_head_revision.get().to_be_bytes());
    digest.update(
        input
            .predecessor_authority_payload_digest
            .as_str()
            .as_bytes(),
    );
    digest.update(&input.payload.revision.get().to_be_bytes());
    digest.update(payload_digest.as_str().as_bytes());
    digest.update(input.created_by_principal_id.as_bytes());
    InstallationAuthorityRequestDigestV1(digest.finalize())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstallationAuthorityIdentityErrorV1 {
    #[error("installation authority identifier is invalid")]
    Identifier,
    #[error("installation authority revision is invalid")]
    Revision,
    #[error("installation authority policy is invalid")]
    Policy,
    #[error("installation authority digest is invalid")]
    Digest,
}

fn bounded_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn canonical_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use desired_state::ResourceKey;
    use discord_model::ChannelId;

    use super::*;
    use crate::{resource_binding_fingerprint_v2, ResourceBindingMap};

    fn binding_fingerprint(channel_id: u64) -> ResourceBindingFingerprint {
        let mut bindings = ResourceBindingMap::default();
        bindings.channel_bindings.insert(
            ResourceKey("community_hub".to_string()),
            ChannelId(channel_id),
        );
        resource_binding_fingerprint_v2(&bindings)
    }

    fn scope<'a>(tenant_id: &'a str, installation_id: &'a str) -> InstallationAuthorityScopeV1<'a> {
        InstallationAuthorityScopeV1::new(tenant_id, installation_id).unwrap()
    }

    fn policy(
        revision: u64,
        required_approvals: u32,
        activation_ttl_seconds: u64,
    ) -> InstallationAuthorityPolicyV1 {
        InstallationAuthorityPolicyV1::new(
            NonZeroU64::new(revision).unwrap(),
            NonZeroU32::new(required_approvals).unwrap(),
            NonZeroU64::new(activation_ttl_seconds).unwrap(),
        )
        .unwrap()
    }

    fn payload<'a>(
        fingerprint: &'a ResourceBindingFingerprint,
    ) -> InstallationAuthorityPayloadIdentityV1<'a> {
        InstallationAuthorityPayloadIdentityV1::new(
            scope("tenant.staging", "installation.staging"),
            NonZeroU64::new(2).unwrap(),
            NonZeroU64::new(2).unwrap(),
            fingerprint,
            policy(1, 1, 86_400),
        )
        .unwrap()
    }

    fn predecessor_digest(value: char) -> InstallationAuthorityPayloadDigestV1 {
        InstallationAuthorityPayloadDigestV1::parse(value.to_string().repeat(64)).unwrap()
    }

    #[test]
    fn authority_and_request_golden_vectors_are_stable() {
        let fingerprint = binding_fingerprint(700);
        let payload = payload(&fingerprint);
        let predecessor = predecessor_digest('a');
        let request = InstallationAuthorityRequestIdentityV1::new(
            NonZeroU64::new(1).unwrap(),
            &predecessor,
            &payload,
            "discord:1056857223529250906",
        )
        .unwrap();

        assert_eq!(
            installation_authority_payload_digest_v1(&payload).as_str(),
            "6a010edde27b5b6c83dd669a96823f31c4e65a99fac4d5736fa5d4ff3c56532b"
        );
        assert_eq!(
            installation_authority_request_digest_v1(&request).as_str(),
            "87fc313ca0571ca7b0a8ff7e03c29144de83bbd8f7f58df92bfb6679ec4743d4"
        );
    }

    #[test]
    fn every_payload_field_and_request_actor_are_identity_bearing() {
        let first_fingerprint = binding_fingerprint(700);
        let second_fingerprint = binding_fingerprint(701);
        let baseline = payload(&first_fingerprint);
        let baseline_digest = installation_authority_payload_digest_v1(&baseline);
        let predecessor = predecessor_digest('a');
        let variants = [
            InstallationAuthorityPayloadIdentityV1::new(
                scope("tenant.other", "installation.staging"),
                NonZeroU64::new(2).unwrap(),
                NonZeroU64::new(2).unwrap(),
                &first_fingerprint,
                policy(1, 1, 86_400),
            )
            .unwrap(),
            InstallationAuthorityPayloadIdentityV1::new(
                scope("tenant.staging", "installation.other"),
                NonZeroU64::new(2).unwrap(),
                NonZeroU64::new(2).unwrap(),
                &first_fingerprint,
                policy(1, 1, 86_400),
            )
            .unwrap(),
            InstallationAuthorityPayloadIdentityV1::new(
                scope("tenant.staging", "installation.staging"),
                NonZeroU64::new(3).unwrap(),
                NonZeroU64::new(2).unwrap(),
                &first_fingerprint,
                policy(1, 1, 86_400),
            )
            .unwrap(),
            InstallationAuthorityPayloadIdentityV1::new(
                scope("tenant.staging", "installation.staging"),
                NonZeroU64::new(2).unwrap(),
                NonZeroU64::new(3).unwrap(),
                &first_fingerprint,
                policy(1, 1, 86_400),
            )
            .unwrap(),
            InstallationAuthorityPayloadIdentityV1::new(
                scope("tenant.staging", "installation.staging"),
                NonZeroU64::new(2).unwrap(),
                NonZeroU64::new(2).unwrap(),
                &second_fingerprint,
                policy(1, 1, 86_400),
            )
            .unwrap(),
            InstallationAuthorityPayloadIdentityV1::new(
                scope("tenant.staging", "installation.staging"),
                NonZeroU64::new(2).unwrap(),
                NonZeroU64::new(2).unwrap(),
                &first_fingerprint,
                policy(2, 1, 86_400),
            )
            .unwrap(),
            InstallationAuthorityPayloadIdentityV1::new(
                scope("tenant.staging", "installation.staging"),
                NonZeroU64::new(2).unwrap(),
                NonZeroU64::new(2).unwrap(),
                &first_fingerprint,
                policy(1, 1, 86_401),
            )
            .unwrap(),
        ];
        for variant in variants {
            assert_ne!(
                installation_authority_payload_digest_v1(&variant),
                baseline_digest
            );
        }

        let request = InstallationAuthorityRequestIdentityV1::new(
            NonZeroU64::new(1).unwrap(),
            &predecessor,
            &baseline,
            "discord:1056857223529250906",
        )
        .unwrap();
        let other_actor = InstallationAuthorityRequestIdentityV1::new(
            NonZeroU64::new(1).unwrap(),
            &predecessor,
            &baseline,
            "discord:1056857223529250907",
        )
        .unwrap();
        assert_ne!(
            installation_authority_request_digest_v1(&request),
            installation_authority_request_digest_v1(&other_actor)
        );

        let other_predecessor = predecessor_digest('b');
        let other_predecessor_request = InstallationAuthorityRequestIdentityV1::new(
            NonZeroU64::new(1).unwrap(),
            &other_predecessor,
            &baseline,
            "discord:1056857223529250906",
        )
        .unwrap();
        assert_ne!(
            installation_authority_request_digest_v1(&request),
            installation_authority_request_digest_v1(&other_predecessor_request)
        );
    }

    #[test]
    fn field_boundaries_and_digest_domains_cannot_alias() {
        let fingerprint = binding_fingerprint(700);
        let joined = InstallationAuthorityPayloadIdentityV1::new(
            scope("ab", "c"),
            NonZeroU64::new(2).unwrap(),
            NonZeroU64::new(2).unwrap(),
            &fingerprint,
            policy(1, 1, 86_400),
        )
        .unwrap();
        let split = InstallationAuthorityPayloadIdentityV1::new(
            scope("a", "bc"),
            NonZeroU64::new(2).unwrap(),
            NonZeroU64::new(2).unwrap(),
            &fingerprint,
            policy(1, 1, 86_400),
        )
        .unwrap();
        assert_ne!(
            installation_authority_payload_digest_v1(&joined),
            installation_authority_payload_digest_v1(&split)
        );

        let fields = [b"same".as_slice()];
        let mut payload_domain = LengthFramedSha256::new(AUTHORITY_PAYLOAD_DOMAIN_V1);
        payload_domain.update(fields[0]);
        let mut request_domain = LengthFramedSha256::new(AUTHORITY_REQUEST_DOMAIN_V1);
        request_domain.update(fields[0]);
        assert_ne!(payload_domain.finalize(), request_domain.finalize());
    }

    #[test]
    fn invalid_identity_and_policy_values_fail_closed() {
        assert_eq!(
            InstallationAuthorityScopeV1::new("invalid tenant", "installation.staging"),
            Err(InstallationAuthorityIdentityErrorV1::Identifier)
        );
        assert_eq!(
            InstallationAuthorityPolicyV1::new(
                NonZeroU64::new(1).unwrap(),
                NonZeroU32::new(2).unwrap(),
                NonZeroU64::new(86_400).unwrap(),
            ),
            Err(InstallationAuthorityIdentityErrorV1::Policy)
        );
        assert!(InstallationAuthorityPayloadDigestV1::parse("A".repeat(64)).is_err());
        assert!(InstallationAuthorityRequestDigestV1::parse("0".repeat(63)).is_err());
    }
}
