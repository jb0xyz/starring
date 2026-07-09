use desired_compiler::NormalizedTarget;
use desired_state::ResourceKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<DiffChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<DiffConflict>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred: Vec<DeferredItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffChange {
    pub op: ChangeOp,
    pub target: DiffTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<ChangedField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    Create,
    Update,
    Delete,
    NoOp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffTarget {
    Role {
        key: ResourceKey,
    },
    Channel {
        key: ResourceKey,
    },
    Overwrite {
        channel: ResourceKey,
        target: NormalizedTarget,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangedField {
    Name,
    Permissions,
    ChannelType,
    Parent,
    Allow,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffConflict {
    pub target: DiffTarget,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferredItem {
    pub kind: String,
    pub key: ResourceKey,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::ResourceKey;

    #[test]
    fn diff_result_roundtrip() {
        let mut diff = DiffResult::default();
        diff.changes.push(DiffChange {
            op: ChangeOp::Create,
            target: DiffTarget::Role {
                key: ResourceKey("r".to_string()),
            },
            changed: vec![],
        });
        let json = serde_json::to_string(&diff).unwrap();
        assert_eq!(serde_json::from_str::<DiffResult>(&json).unwrap(), diff);
    }
}
