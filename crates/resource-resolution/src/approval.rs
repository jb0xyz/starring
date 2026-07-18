use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;

use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, RoleId};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::ResourceBindingMap;

const APPROVAL_BINDING_DOMAIN_V1: &[u8] = b"starring.activation.approval_binding.v1\0";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedApprovalBinding {
    Channel { key: ResourceKey, id: ChannelId },
    Role { key: ResourceKey, id: RoleId },
}

impl ResolvedApprovalBinding {
    fn identity(&self) -> (u8, &ResourceKey) {
        match self {
            Self::Channel { key, .. } => (0, key),
            Self::Role { key, .. } => (1, key),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ApprovalBindingFingerprint(String);

impl ApprovalBindingFingerprint {
    pub fn parse(value: &str) -> Result<Self, ApprovalBindingFingerprintError> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value.to_string()))
        } else {
            Err(ApprovalBindingFingerprintError)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ApprovalBindingFingerprint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ApprovalBindingFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("approval binding fingerprint must be lowercase SHA-256 hexadecimal")]
pub struct ApprovalBindingFingerprintError;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalBindingProjectionError {
    #[error("approval bindings must be strictly sorted by kind and key without duplicates")]
    NonCanonical,
    #[error("required channel binding is missing: {key}")]
    MissingChannel { key: String },
    #[error("required role binding is missing: {key}")]
    MissingRole { key: String },
}

pub fn project_required_bindings(
    required: &[ResolvedApprovalBinding],
    bindings: &ResourceBindingMap,
) -> Result<Vec<ResolvedApprovalBinding>, ApprovalBindingProjectionError> {
    validate_canonical(required)?;
    required
        .iter()
        .map(|binding| match binding {
            ResolvedApprovalBinding::Channel { key, .. } => bindings
                .channel_bindings
                .get(key)
                .copied()
                .map(|id| ResolvedApprovalBinding::Channel {
                    key: key.clone(),
                    id,
                })
                .ok_or_else(|| ApprovalBindingProjectionError::MissingChannel {
                    key: key.0.clone(),
                }),
            ResolvedApprovalBinding::Role { key, .. } => bindings
                .role_bindings
                .get(key)
                .copied()
                .map(|id| ResolvedApprovalBinding::Role {
                    key: key.clone(),
                    id,
                })
                .ok_or_else(|| ApprovalBindingProjectionError::MissingRole { key: key.0.clone() }),
        })
        .collect()
}

pub fn approval_binding_fingerprint_v1(
    guild_id: GuildId,
    revision: NonZeroU64,
    bindings: &[ResolvedApprovalBinding],
) -> Result<ApprovalBindingFingerprint, ApprovalBindingProjectionError> {
    validate_canonical(bindings)?;
    let mut hasher = Sha256::new();
    update_length_framed(&mut hasher, APPROVAL_BINDING_DOMAIN_V1);
    update_length_framed(&mut hasher, guild_id.to_string().as_bytes());
    update_length_framed(&mut hasher, &revision.get().to_be_bytes());
    for binding in bindings {
        match binding {
            ResolvedApprovalBinding::Channel { key, id } => {
                update_length_framed(&mut hasher, b"channel");
                update_length_framed(&mut hasher, key.0.as_bytes());
                update_length_framed(&mut hasher, id.to_string().as_bytes());
            }
            ResolvedApprovalBinding::Role { key, id } => {
                update_length_framed(&mut hasher, b"role");
                update_length_framed(&mut hasher, key.0.as_bytes());
                update_length_framed(&mut hasher, id.to_string().as_bytes());
            }
        }
    }
    Ok(ApprovalBindingFingerprint(to_lower_hex(&hasher.finalize())))
}

fn validate_canonical(
    bindings: &[ResolvedApprovalBinding],
) -> Result<(), ApprovalBindingProjectionError> {
    if bindings
        .windows(2)
        .all(|window| window[0].identity() < window[1].identity())
    {
        Ok(())
    } else {
        Err(ApprovalBindingProjectionError::NonCanonical)
    }
}

fn update_length_framed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("approval binding field exceeds u64::MAX");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

fn to_lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn required() -> Vec<ResolvedApprovalBinding> {
        vec![
            ResolvedApprovalBinding::Channel {
                key: ResourceKey("community_hub".to_string()),
                id: ChannelId(700),
            },
            ResolvedApprovalBinding::Role {
                key: ResourceKey("member".to_string()),
                id: RoleId(800),
            },
        ]
    }

    #[test]
    fn projection_ignores_unrelated_bindings_and_tracks_required_drift() {
        let expected = required();
        let mut current = ResourceBindingMap::default();
        current
            .channel_bindings
            .insert(ResourceKey("community_hub".to_string()), ChannelId(700));
        current
            .channel_bindings
            .insert(ResourceKey("unrelated".to_string()), ChannelId(999));
        current
            .role_bindings
            .insert(ResourceKey("member".to_string()), RoleId(800));
        assert_eq!(
            project_required_bindings(&expected, &current).unwrap(),
            expected
        );
        current
            .role_bindings
            .insert(ResourceKey("member".to_string()), RoleId(801));
        assert_ne!(
            project_required_bindings(&expected, &current).unwrap(),
            expected
        );
    }

    #[test]
    fn canonical_order_and_missing_resources_fail_closed() {
        let mut reversed = required();
        reversed.reverse();
        assert_eq!(
            approval_binding_fingerprint_v1(GuildId(7), NonZeroU64::new(1).unwrap(), &reversed)
                .unwrap_err(),
            ApprovalBindingProjectionError::NonCanonical
        );
        assert!(matches!(
            project_required_bindings(&required(), &ResourceBindingMap::default()),
            Err(ApprovalBindingProjectionError::MissingChannel { .. })
        ));
    }

    #[test]
    fn identity_inputs_are_bound_and_fingerprint_serde_is_strict() {
        let original =
            approval_binding_fingerprint_v1(GuildId(7), NonZeroU64::new(3).unwrap(), &required())
                .unwrap();
        let changed_guild =
            approval_binding_fingerprint_v1(GuildId(8), NonZeroU64::new(3).unwrap(), &required())
                .unwrap();
        let changed_revision =
            approval_binding_fingerprint_v1(GuildId(7), NonZeroU64::new(4).unwrap(), &required())
                .unwrap();
        assert_ne!(original, changed_guild);
        assert_ne!(original, changed_revision);
        assert_eq!(
            serde_json::from_str::<ApprovalBindingFingerprint>(
                &serde_json::to_string(&original).unwrap()
            )
            .unwrap(),
            original
        );
        assert!(ApprovalBindingFingerprint::parse(&"A".repeat(64)).is_err());
    }

    #[test]
    fn v1_golden_vector_is_stable() {
        assert_eq!(
            approval_binding_fingerprint_v1(GuildId(7), NonZeroU64::new(3).unwrap(), &required())
                .unwrap()
                .as_str(),
            "6f2521fc613634eea480c210e12f503b4e046c25e1200f05a8c9051814599d78"
        );
    }
}
