use std::collections::BTreeMap;

use desired_state::ResourceKey;
use discord_model::{ChannelId, RoleId};
use resource_resolution::ResourceBindingMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredResourceBindingsV1 {
    #[serde(default)]
    role_bindings: BTreeMap<String, String>,
    #[serde(default)]
    channel_bindings: BTreeMap<String, String>,
}

pub(crate) fn decode_resource_bindings(value: Value) -> Result<ResourceBindingMap, &'static str> {
    let stored = serde_json::from_value::<StoredResourceBindingsV1>(value)
        .map_err(|_| "persisted resource bindings have an invalid shape")?;
    let mut bindings = ResourceBindingMap::default();
    for (key, value) in stored.role_bindings {
        let id = canonical_snowflake(&value)
            .ok_or("persisted role binding contains an invalid Discord identifier")?;
        bindings.role_bindings.insert(ResourceKey(key), RoleId(id));
    }
    for (key, value) in stored.channel_bindings {
        let id = canonical_snowflake(&value)
            .ok_or("persisted channel binding contains an invalid Discord identifier")?;
        bindings
            .channel_bindings
            .insert(ResourceKey(key), ChannelId(id));
    }
    Ok(bindings)
}

pub(crate) fn encode_resource_bindings(
    bindings: &ResourceBindingMap,
) -> Result<Value, &'static str> {
    let stored = StoredResourceBindingsV1 {
        role_bindings: bindings
            .role_bindings
            .iter()
            .map(|(key, id)| (key.0.clone(), id.to_string()))
            .collect(),
        channel_bindings: bindings
            .channel_bindings
            .iter()
            .map(|(key, id)| (key.0.clone(), id.to_string()))
            .collect(),
    };
    serde_json::to_value(stored).map_err(|_| "resource bindings could not be encoded")
}

fn canonical_snowflake(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    (parsed != 0 && parsed.to_string() == value).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn bindings_decode_with_exact_shape_and_canonical_ids() {
        let bindings = decode_resource_bindings(json!({
            "role_bindings": {"member": "701"},
            "channel_bindings": {"community_hub": "700"}
        }))
        .unwrap();
        assert_eq!(
            bindings.role_bindings[&ResourceKey("member".to_string())],
            RoleId(701)
        );
        assert_eq!(
            bindings.channel_bindings[&ResourceKey("community_hub".to_string())],
            ChannelId(700)
        );
    }

    #[test]
    fn bindings_reject_unknown_fields_and_noncanonical_ids() {
        assert!(decode_resource_bindings(json!({
            "role_bindings": {},
            "channel_bindings": {},
            "tenant_id": "forged"
        }))
        .is_err());
        for id in ["0", "01", "-1", "18446744073709551616"] {
            assert!(decode_resource_bindings(json!({
                "role_bindings": {"member": id},
                "channel_bindings": {}
            }))
            .is_err());
        }
    }

    #[test]
    fn bindings_encode_to_the_exact_canonical_storage_shape() {
        let mut bindings = ResourceBindingMap::default();
        bindings
            .role_bindings
            .insert(ResourceKey("member".to_string()), RoleId(701));
        bindings
            .channel_bindings
            .insert(ResourceKey("community_hub".to_string()), ChannelId(700));
        let encoded = encode_resource_bindings(&bindings).unwrap();
        assert_eq!(
            encoded,
            json!({
                "role_bindings": {"member": "701"},
                "channel_bindings": {"community_hub": "700"}
            })
        );
        assert_eq!(decode_resource_bindings(encoded).unwrap(), bindings);
    }
}
