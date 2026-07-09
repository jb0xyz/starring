use desired_state::ResourceKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceSymbol {
    Role(ResourceKey),
    Channel(ResourceKey),
}
