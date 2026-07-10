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
    pub key: String,
    pub label: String,
}
