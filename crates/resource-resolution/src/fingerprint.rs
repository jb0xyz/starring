use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::ResourceBindingMap;

const RESOURCE_CONTEXT_DOMAIN_V2: &[u8] = b"starring.intent.resource_context.v2\0";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResourceBindingFingerprint(String);

impl ResourceBindingFingerprint {
    pub fn parse(value: &str) -> Result<Self, ResourceBindingFingerprintError> {
        if value.len() == 64
            && value
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            Ok(Self(value.to_string()))
        } else {
            Err(ResourceBindingFingerprintError)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl Display for ResourceBindingFingerprint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ResourceBindingFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("resource binding fingerprint must be lowercase SHA-256 hexadecimal")]
pub struct ResourceBindingFingerprintError;

pub fn resource_binding_fingerprint_v2(
    bindings: &ResourceBindingMap,
) -> ResourceBindingFingerprint {
    let mut digest = LengthFramedDigest::new(RESOURCE_CONTEXT_DOMAIN_V2);
    for (key, id) in &bindings.channel_bindings {
        digest.update(b"channel");
        digest.update(key.0.as_bytes());
        digest.update(id.to_string().as_bytes());
    }
    for (key, id) in &bindings.role_bindings {
        digest.update(b"role");
        digest.update(key.0.as_bytes());
        digest.update(id.to_string().as_bytes());
    }
    ResourceBindingFingerprint(digest.finalize())
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

    fn update(&mut self, field: &[u8]) {
        update_length_framed(&mut self.hasher, field);
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

fn update_length_framed(hasher: &mut Sha256, bytes: &[u8]) {
    let length = u64::try_from(bytes.len()).expect("binding fingerprint field exceeds u64::MAX");
    hasher.update(length.to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use desired_state::ResourceKey;
    use discord_model::{ChannelId, RoleId};

    use super::*;

    fn bindings() -> ResourceBindingMap {
        let mut bindings = ResourceBindingMap::default();
        bindings
            .channel_bindings
            .insert(ResourceKey("community_hub".to_string()), ChannelId(700));
        bindings
    }

    #[test]
    fn v2_golden_vector_matches_the_intent_context_contract() {
        assert_eq!(
            resource_binding_fingerprint_v2(&bindings()).as_str(),
            "27c51a7b90c32b1fd4095deefe2f48cfdda9f41416f5208cc683e8adf42418d4"
        );
    }

    #[test]
    fn key_id_and_kind_changes_have_distinct_fingerprints() {
        let original = resource_binding_fingerprint_v2(&bindings());

        let mut different_key = ResourceBindingMap::default();
        different_key
            .channel_bindings
            .insert(ResourceKey("other_hub".to_string()), ChannelId(700));

        let mut different_id = bindings();
        different_id
            .channel_bindings
            .insert(ResourceKey("community_hub".to_string()), ChannelId(701));

        let mut different_kind = ResourceBindingMap::default();
        different_kind
            .role_bindings
            .insert(ResourceKey("community_hub".to_string()), RoleId(700));

        for changed in [different_key, different_id, different_kind] {
            assert_ne!(resource_binding_fingerprint_v2(&changed), original);
        }
    }

    #[test]
    fn persisted_fingerprints_validate_strict_lowercase_sha256() {
        let fingerprint = resource_binding_fingerprint_v2(&bindings());
        assert_eq!(
            ResourceBindingFingerprint::parse(fingerprint.as_str()).unwrap(),
            fingerprint
        );
        assert!(ResourceBindingFingerprint::parse(&"A".repeat(64)).is_err());
        assert!(ResourceBindingFingerprint::parse("short").is_err());
        assert_eq!(
            serde_json::from_str::<ResourceBindingFingerprint>(
                &serde_json::to_string(&fingerprint).unwrap()
            )
            .unwrap(),
            fingerprint
        );
        assert!(serde_json::from_str::<ResourceBindingFingerprint>(&format!(
            "\"{}\"",
            "A".repeat(64)
        ))
        .is_err());
    }
}
