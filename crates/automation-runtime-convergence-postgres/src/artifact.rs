use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetSchemaVersion, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_state::InteractionRuleSet;
use serde::Deserialize;
use serde_json::Value;
use sqlx::types::Json;

#[derive(sqlx::FromRow, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeTargetArtifactRow {
    pub schema_version: i64,
    pub definition: Option<Json<Value>>,
    pub content_hash: String,
    pub canonical_content_hash: Option<String>,
}

pub(crate) fn runtime_target_artifact_is_valid(
    artifact: &RuntimeTargetArtifactRow,
    expected_hash: &RuleSetContentHash,
) -> bool {
    let Some(schema_version) = u32::try_from(artifact.schema_version)
        .ok()
        .and_then(|value| RuleSetSchemaVersion::new(value).ok())
    else {
        return false;
    };
    if schema_version != CURRENT_RULESET_SCHEMA_VERSION {
        return false;
    }
    let Some(definition) = artifact.definition.as_ref().and_then(|definition| {
        serde_json::from_value::<InteractionRuleSet>(definition.0.clone()).ok()
    }) else {
        return false;
    };
    artifact.canonical_content_hash.as_deref() == Some(artifact.content_hash.as_str())
        && RuleSetContentHash::parse_hex(&artifact.content_hash).as_ref() == Some(expected_hash)
        && automation_core::validate_structural(&definition).is_ok()
        && content_hash(schema_version, &definition).ok().as_ref() == Some(expected_hash)
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{content_hash, RuleSetSchemaVersion, CURRENT_RULESET_SCHEMA_VERSION};
    use automation_state::InteractionRuleSet;
    use sqlx::types::Json;

    use super::{runtime_target_artifact_is_valid, RuntimeTargetArtifactRow};

    fn artifact(
        schema_version: RuleSetSchemaVersion,
    ) -> (
        RuntimeTargetArtifactRow,
        automation_ruleset::RuleSetContentHash,
    ) {
        let definition = InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: Vec::new(),
        };
        let expected_hash = content_hash(schema_version, &definition).unwrap();
        let content_hash = expected_hash.to_hex();
        (
            RuntimeTargetArtifactRow {
                schema_version: i64::from(schema_version.get()),
                definition: Some(Json(serde_json::to_value(definition).unwrap())),
                content_hash: content_hash.clone(),
                canonical_content_hash: Some(content_hash),
            },
            expected_hash,
        )
    }

    #[test]
    fn artifact_verifier_rejects_self_consistent_unsupported_schema() {
        let (current, current_hash) = artifact(CURRENT_RULESET_SCHEMA_VERSION);
        assert!(runtime_target_artifact_is_valid(&current, &current_hash));
        let (future, future_hash) =
            artifact(RuleSetSchemaVersion::new(CURRENT_RULESET_SCHEMA_VERSION.get() + 1).unwrap());
        assert!(!runtime_target_artifact_is_valid(&future, &future_hash));
    }
}
