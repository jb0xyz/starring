use std::collections::BTreeMap;
use std::fmt;

use automation_state::InteractionRuleSet;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::version::RuleSetSchemaVersion;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleSetContentHash([u8; 32]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSetHashError {
    Serialization(String),
}

impl RuleSetContentHash {
    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }

    pub fn parse_hex(value: &str) -> Option<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in value.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16)? as u8;
            let lo = (chunk[1] as char).to_digit(16)? as u8;
            bytes[i] = (hi << 4) | lo;
        }
        Some(RuleSetContentHash(bytes))
    }
}

impl fmt::Display for RuleSetContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for RuleSetContentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for RuleSetContentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        RuleSetContentHash::parse_hex(&value)
            .ok_or_else(|| serde::de::Error::custom("expected 64-char lowercase hex"))
    }
}

#[derive(Serialize)]
struct RuleSetHashInput<'a> {
    schema_version: RuleSetSchemaVersion,
    definition: &'a InteractionRuleSet,
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k, canonicalize(v));
            }
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        other => other,
    }
}

pub fn content_hash(
    schema_version: RuleSetSchemaVersion,
    definition: &InteractionRuleSet,
) -> Result<RuleSetContentHash, RuleSetHashError> {
    let input = RuleSetHashInput {
        schema_version,
        definition,
    };
    let value =
        serde_json::to_value(&input).map_err(|e| RuleSetHashError::Serialization(e.to_string()))?;
    let bytes = serde_json::to_vec(&canonicalize(value))
        .map_err(|e| RuleSetHashError::Serialization(e.to_string()))?;
    let digest = Sha256::digest(&bytes);
    Ok(RuleSetContentHash(digest.into()))
}

pub trait RuleSetHasher {
    fn hash(
        &self,
        schema_version: RuleSetSchemaVersion,
        definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError>;
}

#[derive(Default)]
pub struct Sha256RuleSetHasher;

impl RuleSetHasher for Sha256RuleSetHasher {
    fn hash(
        &self,
        schema_version: RuleSetSchemaVersion,
        definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError> {
        content_hash(schema_version, definition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::CURRENT_RULESET_SCHEMA_VERSION;
    use automation_state::{
        ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, RoleRef, TriggerSpec,
    };
    use desired_state::ResourceKey;

    fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "join".to_string(),
                },
                actions,
            }],
        }
    }

    fn grant() -> ActionSpec {
        ActionSpec::GrantRole {
            role: RoleRef::Existing(ResourceKey("m".to_string())),
            target: ActionTarget::Actor,
        }
    }

    fn respond() -> ActionSpec {
        ActionSpec::RespondEphemeral {
            content: "hi".to_string(),
        }
    }

    #[test]
    fn same_definition_same_hash() {
        let a = content_hash(
            CURRENT_RULESET_SCHEMA_VERSION,
            &ruleset(vec![grant(), respond()]),
        )
        .unwrap();
        let b = content_hash(
            CURRENT_RULESET_SCHEMA_VERSION,
            &ruleset(vec![grant(), respond()]),
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn action_order_changes_hash() {
        let a = content_hash(
            CURRENT_RULESET_SCHEMA_VERSION,
            &ruleset(vec![grant(), respond()]),
        )
        .unwrap();
        let b = content_hash(
            CURRENT_RULESET_SCHEMA_VERSION,
            &ruleset(vec![respond(), grant()]),
        )
        .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn schema_version_changes_hash() {
        let v1 = RuleSetSchemaVersion::new(1).unwrap();
        let v2 = RuleSetSchemaVersion::new(2).unwrap();
        let a = content_hash(v1, &ruleset(vec![grant()])).unwrap();
        let b = content_hash(v2, &ruleset(vec![grant()])).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn legacy_modal_definition_preserves_wire_shape_and_content_hash() {
        let legacy = r#"{"version":1,"panels":[],"modals":[{"key":"room","title":"Room","fields":[{"key":"name","label":"Name","style":"short","required":true}]}],"rules":[]}"#;
        let raw_definition: Value = serde_json::from_str(legacy).unwrap();
        let definition: InteractionRuleSet = serde_json::from_str(legacy).unwrap();
        let raw_input = serde_json::json!({
            "schema_version": CURRENT_RULESET_SCHEMA_VERSION,
            "definition": raw_definition.clone(),
        });
        let raw_bytes = serde_json::to_vec(&canonicalize(raw_input)).unwrap();
        let expected = RuleSetContentHash(Sha256::digest(raw_bytes).into());

        assert_eq!(serde_json::to_value(&definition).unwrap(), raw_definition);
        assert_eq!(
            content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition).unwrap(),
            expected
        );
    }

    #[test]
    fn hex_roundtrip_and_validation() {
        let h = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset(vec![grant()])).unwrap();
        let hex = h.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(RuleSetContentHash::parse_hex(&hex), Some(h));
        assert_eq!(RuleSetContentHash::parse_hex("XYZ"), None);
        assert_eq!(RuleSetContentHash::parse_hex(&hex.to_uppercase()), None);
    }
}
