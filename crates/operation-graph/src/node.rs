use desired_compiler::NormalizedTarget;
use desired_state::ResourceKey;
use discord_model::{ChannelType, Permissions};
use serde::{Deserialize, Serialize};

use crate::symbol::ResourceSymbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OpId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    CreateRole {
        key: ResourceKey,
        name: Option<String>,
        permissions: Option<Permissions>,
    },
    UpdateRole {
        key: ResourceKey,
        name: Option<String>,
        permissions: Option<Permissions>,
    },
    DeleteRole {
        key: ResourceKey,
    },
    CreateChannel {
        key: ResourceKey,
        name: Option<String>,
        channel_type: Option<ChannelType>,
        parent: Option<ResourceKey>,
    },
    UpdateChannel {
        key: ResourceKey,
        name: Option<String>,
        channel_type: Option<ChannelType>,
    },
    DeleteChannel {
        key: ResourceKey,
    },
    CreateOverwrite {
        channel: ResourceKey,
        target: NormalizedTarget,
        allow: Permissions,
        deny: Permissions,
    },
    UpdateOverwrite {
        channel: ResourceKey,
        target: NormalizedTarget,
        allow: Permissions,
        deny: Permissions,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationNode {
    pub id: OpId,
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub produces: Vec<ResourceSymbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumes: Vec<ResourceSymbol>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<OpId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationGraph {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<OperationNode>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::ResourceKey;
    use discord_model::Permissions;

    #[test]
    fn node_roundtrip() {
        let node = OperationNode {
            id: OpId(0),
            operation: Operation::CreateRole {
                key: ResourceKey("r".to_string()),
                name: Some("R".to_string()),
                permissions: Some(Permissions::empty()),
            },
            produces: vec![ResourceSymbol::Role(ResourceKey("r".to_string()))],
            consumes: vec![],
            depends_on: vec![],
        };
        let json = serde_json::to_string(&node).unwrap();
        assert_eq!(serde_json::from_str::<OperationNode>(&json).unwrap(), node);
    }
}
