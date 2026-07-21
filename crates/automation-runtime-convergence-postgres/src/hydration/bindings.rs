use std::collections::BTreeMap;

use desired_state::ResourceKey;
use discord_model::{ChannelId, RoleId};
use resource_resolution::ResourceBindingMap;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredResourceBindingsV1 {
    #[serde(default)]
    role_bindings: BTreeMap<String, String>,
    #[serde(default)]
    channel_bindings: BTreeMap<String, String>,
}

pub(super) fn decode_resource_bindings(value: Value) -> Result<ResourceBindingMap, &'static str> {
    let stored = serde_json::from_value::<StoredResourceBindingsV1>(value)
        .map_err(|_| "runtime resource bindings shape")?;
    let mut bindings = ResourceBindingMap::default();
    for (key, value) in stored.role_bindings {
        let id = canonical_snowflake(&value).ok_or("runtime role binding")?;
        bindings.role_bindings.insert(ResourceKey(key), RoleId(id));
    }
    for (key, value) in stored.channel_bindings {
        let id = canonical_snowflake(&value).ok_or("runtime channel binding")?;
        bindings
            .channel_bindings
            .insert(ResourceKey(key), ChannelId(id));
    }
    Ok(bindings)
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
    fn exact_shape_and_canonical_snowflakes_are_required() {
        assert!(decode_resource_bindings(json!({
            "role_bindings": {"member": "701"},
            "channel_bindings": {"hub": "700"}
        }))
        .is_ok());
        for value in [
            json!({"role_bindings": {}, "channel_bindings": {}, "extra": true}),
            json!({"role_bindings": {"member": "0"}, "channel_bindings": {}}),
            json!({"role_bindings": {}, "channel_bindings": {"hub": "01"}}),
        ] {
            assert!(decode_resource_bindings(value).is_err());
        }
    }
}
