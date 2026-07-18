use std::num::{NonZeroU32, NonZeroU64};

use automation_ruleset::RuleSetContentHash;
use chrono::{DateTime, Utc};
use discord_model::UserId;
use resource_resolution::{
    approval_binding_fingerprint_v1, ApprovalBindingFingerprint, ResolvedApprovalBinding,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ActivationDigest, ActivationPromotionId, ActivationRequestId, ActivationTarget, ObservedActive,
};

const APPROVAL_POLICY_DOMAIN_V1: &[u8] = b"starring.activation.approval_policy.v1\0";
const APPROVAL_CONTEXT_DOMAIN_V1: &[u8] = b"starring.activation.approval_context.v1\0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedActiveBaselineV1 {
    Absent,
    Exact {
        version: automation_ruleset::RuleSetVersionId,
        content_hash: RuleSetContentHash,
    },
}

impl ExpectedActiveBaselineV1 {
    pub fn from_observed(observed: Option<&ObservedActive>) -> Self {
        match observed {
            Some(observed) => Self::Exact {
                version: observed.version,
                content_hash: observed.content_hash,
            },
            None => Self::Absent,
        }
    }

    pub fn as_observed(&self) -> Option<ObservedActive> {
        match self {
            Self::Absent => None,
            Self::Exact {
                version,
                content_hash,
            } => Some(ObservedActive {
                version: *version,
                content_hash: *content_hash,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalPolicyBindingV1 {
    pub revision: NonZeroU64,
    pub required_approvals: NonZeroU32,
    pub ttl_seconds: NonZeroU64,
    pub digest: ActivationDigest,
}

impl ApprovalPolicyBindingV1 {
    pub fn validate(&self) -> bool {
        approval_policy_digest_v1(self.revision, self.required_approvals, self.ttl_seconds)
            == self.digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalBindingContextV1 {
    pub revision: NonZeroU64,
    pub required_bindings: Vec<ResolvedApprovalBinding>,
    pub fingerprint: ApprovalBindingFingerprint,
}

impl ApprovalBindingContextV1 {
    pub fn validate(&self, guild_id: discord_model::GuildId) -> bool {
        approval_binding_fingerprint_v1(guild_id, self.revision, &self.required_bindings)
            .is_ok_and(|fingerprint| fingerprint == self.fingerprint)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductApprovalContextV1 {
    pub promotion_id: ActivationPromotionId,
    pub promotion_request_digest: ActivationDigest,
    pub approval_payload_digest: ActivationDigest,
    pub approval_context_digest: ActivationDigest,
    pub binding: ApprovalBindingContextV1,
    pub baseline: ExpectedActiveBaselineV1,
    pub policy: ApprovalPolicyBindingV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "authority", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivationApprovalContextV1 {
    LegacyManual,
    ProductAuthoring {
        context: Box<ProductApprovalContextV1>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivationLinkStateV1 {
    NotRequired,
    Unlinked,
    Linked { linked_at: DateTime<Utc> },
}

pub fn approval_policy_digest_v1(
    revision: NonZeroU64,
    required_approvals: NonZeroU32,
    ttl_seconds: NonZeroU64,
) -> ActivationDigest {
    let mut digest = LengthFramedDigest::new(APPROVAL_POLICY_DOMAIN_V1);
    digest.update(&revision.get().to_be_bytes());
    digest.update(&required_approvals.get().to_be_bytes());
    digest.update(&ttl_seconds.get().to_be_bytes());
    ActivationDigest::parse(&digest.finalize()).expect("SHA-256 digest is a valid identity")
}

pub fn product_approval_context_digest_v1(
    request_id: &ActivationRequestId,
    target: &ActivationTarget,
    requester: UserId,
    context: &ProductApprovalContextV1,
) -> ActivationDigest {
    let mut digest = LengthFramedDigest::new(APPROVAL_CONTEXT_DOMAIN_V1);
    digest.update(request_id.as_str().as_bytes());
    digest.update(target.guild_id.to_string().as_bytes());
    digest.update(target.ruleset_key.as_str().as_bytes());
    digest.update(&target.version.get().to_be_bytes());
    digest.update(target.content_hash.to_hex().as_bytes());
    digest.update(requester.to_string().as_bytes());
    digest.update(context.promotion_id.as_str().as_bytes());
    digest.update(context.promotion_request_digest.as_str().as_bytes());
    digest.update(context.approval_payload_digest.as_str().as_bytes());
    digest.update(&context.binding.revision.get().to_be_bytes());
    digest.update(context.binding.fingerprint.as_str().as_bytes());
    for binding in &context.binding.required_bindings {
        match binding {
            ResolvedApprovalBinding::Channel { key, id } => {
                digest.update(b"channel");
                digest.update(key.0.as_bytes());
                digest.update(id.to_string().as_bytes());
            }
            ResolvedApprovalBinding::Role { key, id } => {
                digest.update(b"role");
                digest.update(key.0.as_bytes());
                digest.update(id.to_string().as_bytes());
            }
        }
    }
    match &context.baseline {
        ExpectedActiveBaselineV1::Absent => digest.update(b"absent"),
        ExpectedActiveBaselineV1::Exact {
            version,
            content_hash,
        } => {
            digest.update(b"exact");
            digest.update(&version.get().to_be_bytes());
            digest.update(content_hash.to_hex().as_bytes());
        }
    }
    digest.update(&context.policy.revision.get().to_be_bytes());
    digest.update(&context.policy.required_approvals.get().to_be_bytes());
    digest.update(&context.policy.ttl_seconds.get().to_be_bytes());
    digest.update(context.policy.digest.as_str().as_bytes());
    ActivationDigest::parse(&digest.finalize()).expect("SHA-256 digest is a valid identity")
}

struct LengthFramedDigest {
    hasher: Sha256,
}

impl LengthFramedDigest {
    fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        update_length_framed(&mut hasher, domain);
        Self { hasher }
    }

    fn update(&mut self, value: &[u8]) {
        update_length_framed(&mut self.hasher, value);
    }

    fn finalize(self) -> String {
        let bytes = self.hasher.finalize();
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write;
            write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
        }
        output
    }
}

fn update_length_framed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("approval context field exceeds u64::MAX");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetKey, RuleSetVersionId};
    use discord_model::GuildId;

    use super::*;

    fn digest(value: char) -> ActivationDigest {
        ActivationDigest::parse(&value.to_string().repeat(64)).unwrap()
    }

    fn context() -> ProductApprovalContextV1 {
        let revision = NonZeroU64::new(3).unwrap();
        let required_bindings = Vec::new();
        ProductApprovalContextV1 {
            promotion_id: ActivationPromotionId::parse(&"a".repeat(64)).unwrap(),
            promotion_request_digest: digest('b'),
            approval_payload_digest: digest('c'),
            approval_context_digest: digest('d'),
            binding: ApprovalBindingContextV1 {
                revision,
                fingerprint: approval_binding_fingerprint_v1(
                    GuildId(7),
                    revision,
                    &required_bindings,
                )
                .unwrap(),
                required_bindings,
            },
            baseline: ExpectedActiveBaselineV1::Absent,
            policy: ApprovalPolicyBindingV1 {
                revision: NonZeroU64::new(5).unwrap(),
                required_approvals: NonZeroU32::new(2).unwrap(),
                ttl_seconds: NonZeroU64::new(1_800).unwrap(),
                digest: approval_policy_digest_v1(
                    NonZeroU64::new(5).unwrap(),
                    NonZeroU32::new(2).unwrap(),
                    NonZeroU64::new(1_800).unwrap(),
                ),
            },
        }
    }

    #[test]
    fn policy_digest_is_stable_and_input_bound() {
        let original = approval_policy_digest_v1(
            NonZeroU64::new(5).unwrap(),
            NonZeroU32::new(2).unwrap(),
            NonZeroU64::new(1_800).unwrap(),
        );
        assert_eq!(
            original.as_str(),
            "52ea6b41f445854786cc0764ee2575de3feefeb693b900412eb6841b7f3308cf"
        );
        assert_ne!(
            original,
            approval_policy_digest_v1(
                NonZeroU64::new(6).unwrap(),
                NonZeroU32::new(2).unwrap(),
                NonZeroU64::new(1_800).unwrap(),
            )
        );
    }

    #[test]
    fn product_context_digest_is_stable_and_input_bound() {
        let context = context();
        let request_id = ActivationRequestId::parse("approval_context_fixture").unwrap();
        let target = ActivationTarget {
            guild_id: GuildId(7),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::new(2).unwrap(),
            content_hash: RuleSetContentHash::parse_hex(&"e".repeat(64)).unwrap(),
        };
        let original =
            product_approval_context_digest_v1(&request_id, &target, UserId(10), &context);
        assert_eq!(
            original.as_str(),
            "ca4049efaf266dad58d3e58301d25f83a45d25d3a63dada9ec6022ef71b609e3"
        );
        assert_ne!(
            original,
            product_approval_context_digest_v1(&request_id, &target, UserId(11), &context)
        );
    }

    #[test]
    fn hash_identities_reject_noncanonical_serde() {
        assert!(
            serde_json::from_str::<ActivationDigest>(&format!("\"{}\"", "A".repeat(64))).is_err()
        );
        assert!(serde_json::from_str::<ActivationPromotionId>("\"short\"").is_err());
    }
}
