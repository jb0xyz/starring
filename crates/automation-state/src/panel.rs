use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    pub key: String,
    pub channel: ResourceKey,
    pub content: String,
    #[serde(default)]
    pub buttons: Vec<ButtonSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonSpec {
    pub label: String,
    pub route: ButtonRoute,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ButtonRoute {
    Static {
        key: String,
    },
    InstanceAction {
        instance: crate::rule::InstanceRef,
        action: String,
    },
}
