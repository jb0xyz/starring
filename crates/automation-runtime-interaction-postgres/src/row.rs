use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceResources, InstanceRuleSetVersion,
    InstanceStatus,
};
use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion, RuleSetVersion,
    RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_ruleset_dispatch::ResolvedPinnedInstanceV1;
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use serde_json::Value;
use sqlx::types::Json;

use crate::RuntimeInteractionPersistenceErrorV1;

#[derive(sqlx::FromRow)]
pub(crate) struct InstanceRowV1 {
    pub(crate) guild_id: String,
    pub(crate) instance_id: String,
    pub(crate) ruleset_key: String,
    pub(crate) ruleset_version: i64,
    pub(crate) kind: String,
    pub(crate) created_by: String,
    pub(crate) status: String,
    pub(crate) resources: Json<Value>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct PinnedInstanceRowV1 {
    pub(crate) guild_id: String,
    pub(crate) instance_id: String,
    pub(crate) ruleset_key: String,
    pub(crate) ruleset_version: i64,
    pub(crate) kind: String,
    pub(crate) created_by: String,
    pub(crate) status: String,
    pub(crate) resources: Json<Value>,
    pub(crate) artifact_found: bool,
    pub(crate) artifact_schema_version: Option<i64>,
    pub(crate) artifact_definition: Option<Json<Value>>,
    pub(crate) artifact_content_hash: Option<String>,
    pub(crate) artifact_created_by: Option<String>,
}

#[derive(Debug)]
pub(crate) enum PinnedInstanceRowOutcomeV1 {
    Resolved(Box<ResolvedPinnedInstanceV1>),
    Inactive(InstanceStatus),
    PinnedVersionMissing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PinnedInstanceRowErrorV1 {
    Instance,
    Artifact,
}

impl InstanceRowV1 {
    pub(crate) fn decode(
        self,
        expected_guild_id: GuildId,
        expected_instance_id: &InstanceId,
    ) -> Result<AutomationInstance, RuntimeInteractionPersistenceErrorV1> {
        decode_instance(
            self.guild_id,
            self.instance_id,
            self.ruleset_key,
            self.ruleset_version,
            self.kind,
            self.created_by,
            self.status,
            self.resources,
            expected_guild_id,
            expected_instance_id,
        )
    }

    pub(crate) fn decode_retryable(
        self,
        expected_guild_id: GuildId,
    ) -> Result<AutomationInstance, RuntimeInteractionPersistenceErrorV1> {
        let expected_instance_id = InstanceId::parse(&self.instance_id)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let instance = self.decode(expected_guild_id, &expected_instance_id)?;
        if instance.status != InstanceStatus::Deleting {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(instance)
    }
}

impl PinnedInstanceRowV1 {
    pub(crate) fn decode(
        self,
        expected_guild_id: GuildId,
        expected_instance_id: &InstanceId,
    ) -> Result<PinnedInstanceRowOutcomeV1, PinnedInstanceRowErrorV1> {
        let instance = decode_instance(
            self.guild_id,
            self.instance_id,
            self.ruleset_key,
            self.ruleset_version,
            self.kind,
            self.created_by,
            self.status,
            self.resources,
            expected_guild_id,
            expected_instance_id,
        )
        .map_err(|_| PinnedInstanceRowErrorV1::Instance)?;
        if instance.status != InstanceStatus::Active {
            return Ok(PinnedInstanceRowOutcomeV1::Inactive(instance.status));
        }
        let artifact_fields = [
            self.artifact_schema_version.is_some(),
            self.artifact_definition.is_some(),
            self.artifact_content_hash.is_some(),
            self.artifact_created_by.is_some(),
        ];
        if !self.artifact_found {
            if artifact_fields.into_iter().any(|present| present) {
                return Err(PinnedInstanceRowErrorV1::Artifact);
            }
            return Ok(PinnedInstanceRowOutcomeV1::PinnedVersionMissing);
        }
        if artifact_fields.into_iter().any(|present| !present) {
            return Err(PinnedInstanceRowErrorV1::Artifact);
        }
        let schema_version = self
            .artifact_schema_version
            .and_then(|value| u32::try_from(value).ok())
            .and_then(|value| RuleSetSchemaVersion::new(value).ok())
            .filter(|version| *version == CURRENT_RULESET_SCHEMA_VERSION)
            .ok_or(PinnedInstanceRowErrorV1::Artifact)?;
        let definition_value = self
            .artifact_definition
            .ok_or(PinnedInstanceRowErrorV1::Artifact)?
            .0;
        if serde_json::to_vec(&definition_value)
            .map_err(|_| PinnedInstanceRowErrorV1::Artifact)?
            .len()
            > 524_288
        {
            return Err(PinnedInstanceRowErrorV1::Artifact);
        }
        let definition = serde_json::from_value::<InteractionRuleSet>(definition_value)
            .map_err(|_| PinnedInstanceRowErrorV1::Artifact)?;
        automation_core::validate_structural(&definition)
            .map_err(|_| PinnedInstanceRowErrorV1::Artifact)?;
        let persisted_hash = RuleSetContentHash::parse_hex(
            &self
                .artifact_content_hash
                .ok_or(PinnedInstanceRowErrorV1::Artifact)?,
        )
        .ok_or(PinnedInstanceRowErrorV1::Artifact)?;
        let recomputed_hash = content_hash(schema_version, &definition)
            .map_err(|_| PinnedInstanceRowErrorV1::Artifact)?;
        if persisted_hash != recomputed_hash {
            return Err(PinnedInstanceRowErrorV1::Artifact);
        }
        let artifact_created_by_text = self
            .artifact_created_by
            .ok_or(PinnedInstanceRowErrorV1::Artifact)?;
        let artifact_created_by = artifact_created_by_text
            .parse::<UserId>()
            .map_err(|_| PinnedInstanceRowErrorV1::Artifact)?;
        if artifact_created_by.0 == 0 || artifact_created_by.to_string() != artifact_created_by_text
        {
            return Err(PinnedInstanceRowErrorV1::Artifact);
        }
        let ruleset_key = RuleSetKey::parse(&instance.ruleset_key)
            .map_err(|_| PinnedInstanceRowErrorV1::Instance)?;
        let version = RuleSetVersionId::new(instance.ruleset_version.get())
            .map_err(|_| PinnedInstanceRowErrorV1::Instance)?;
        Ok(PinnedInstanceRowOutcomeV1::Resolved(Box::new(
            ResolvedPinnedInstanceV1 {
                artifact: RuleSetVersion {
                    guild_id: instance.guild_id,
                    ruleset_key,
                    version,
                    schema_version,
                    definition,
                    content_hash: persisted_hash,
                    created_by: artifact_created_by,
                },
                instance,
            },
        )))
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_instance(
    guild_id: String,
    instance_id: String,
    ruleset_key: String,
    ruleset_version: i64,
    kind: String,
    created_by: String,
    status: String,
    resources: Json<Value>,
    expected_guild_id: GuildId,
    expected_instance_id: &InstanceId,
) -> Result<AutomationInstance, RuntimeInteractionPersistenceErrorV1> {
    let invalid = || RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt;
    let guild_id_text = guild_id;
    let guild_id = guild_id_text.parse::<GuildId>().map_err(|_| invalid())?;
    let instance_id = InstanceId::parse(&instance_id).map_err(|_| invalid())?;
    RuleSetKey::parse(&ruleset_key).map_err(|_| invalid())?;
    let ruleset_version = u32::try_from(ruleset_version)
        .ok()
        .and_then(|value| InstanceRuleSetVersion::new(value).ok())
        .ok_or_else(invalid)?;
    let created_by_text = created_by;
    let created_by = created_by_text.parse::<UserId>().map_err(|_| invalid())?;
    let status = match status.as_str() {
        "active" => InstanceStatus::Active,
        "deleting" => InstanceStatus::Deleting,
        "disabled" => InstanceStatus::Disabled,
        "deleted" => InstanceStatus::Deleted,
        _ => return Err(invalid()),
    };
    let resources =
        serde_json::from_value::<InstanceResources>(resources.0).map_err(|_| invalid())?;
    if guild_id.0 == 0
        || expected_guild_id.0 == 0
        || guild_id.to_string() != guild_id_text
        || created_by.0 == 0
        || created_by.to_string() != created_by_text
        || kind.is_empty()
        || kind.len() > 128
        || !resources_have_nonzero_ids(&resources)
        || serde_json::to_vec(&resources).map_err(|_| invalid())?.len() > 262_144
        || guild_id != expected_guild_id
        || &instance_id != expected_instance_id
    {
        return Err(invalid());
    }
    Ok(AutomationInstance {
        id: instance_id,
        guild_id,
        ruleset_key,
        ruleset_version,
        kind: InstanceKind(kind),
        created_by,
        resources,
        status,
    })
}

fn resources_have_nonzero_ids(resources: &InstanceResources) -> bool {
    resources
        .roles
        .iter()
        .all(|(key, role)| valid_resource_key(key) && role.0 != 0)
        && resources
            .channels
            .iter()
            .all(|(key, channel)| valid_resource_key(key) && channel.0 != 0)
        && resources.messages.iter().all(|(key, message)| {
            valid_resource_key(key) && message.channel.0 != 0 && message.id.0 != 0
        })
}

fn valid_resource_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{content_hash, CURRENT_RULESET_SCHEMA_VERSION};
    use automation_state::InteractionRuleSet;
    use serde_json::json;

    use super::*;

    fn definition() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: Vec::new(),
        }
    }

    fn pinned_row(status: &str) -> PinnedInstanceRowV1 {
        let definition = definition();
        let hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition)
            .unwrap()
            .to_hex();
        PinnedInstanceRowV1 {
            guild_id: "7".to_string(),
            instance_id: "room_1".to_string(),
            ruleset_key: "studyroom".to_string(),
            ruleset_version: 3,
            kind: "studyroom".to_string(),
            created_by: "9".to_string(),
            status: status.to_string(),
            resources: Json(json!({})),
            artifact_found: true,
            artifact_schema_version: Some(i64::from(CURRENT_RULESET_SCHEMA_VERSION.get())),
            artifact_definition: Some(Json(serde_json::to_value(definition).unwrap())),
            artifact_content_hash: Some(hash),
            artifact_created_by: Some("11".to_string()),
        }
    }

    #[test]
    fn inactive_instance_precedes_incomplete_artifact() {
        let mut row = pinned_row("disabled");
        row.artifact_content_hash = None;
        assert!(matches!(
            row.decode(GuildId(7), &InstanceId::parse("room_1").unwrap()),
            Ok(PinnedInstanceRowOutcomeV1::Inactive(
                InstanceStatus::Disabled
            ))
        ));
    }

    #[test]
    fn active_instance_resolves_only_exact_verified_pin() {
        let resolved = match pinned_row("active")
            .decode(GuildId(7), &InstanceId::parse("room_1").unwrap())
            .unwrap()
        {
            PinnedInstanceRowOutcomeV1::Resolved(resolved) => resolved,
            _ => panic!(),
        };
        assert_eq!(resolved.instance.guild_id, GuildId(7));
        assert_eq!(resolved.instance.ruleset_version.get(), 3);
        assert_eq!(resolved.artifact.guild_id, GuildId(7));
        assert_eq!(resolved.artifact.ruleset_key.as_str(), "studyroom");
        assert_eq!(resolved.artifact.version.get(), 3);
    }

    #[test]
    fn active_instance_rejects_tampered_artifact() {
        let mut row = pinned_row("active");
        row.artifact_content_hash = Some("0".repeat(64));
        assert!(matches!(
            row.decode(GuildId(7), &InstanceId::parse("room_1").unwrap()),
            Err(PinnedInstanceRowErrorV1::Artifact)
        ));
    }

    #[test]
    fn route_identity_mismatch_is_corruption() {
        let row = InstanceRowV1 {
            guild_id: "8".to_string(),
            instance_id: "room_1".to_string(),
            ruleset_key: "studyroom".to_string(),
            ruleset_version: 3,
            kind: "studyroom".to_string(),
            created_by: "9".to_string(),
            status: "active".to_string(),
            resources: Json(json!({})),
        };
        assert_eq!(
            row.decode(GuildId(7), &InstanceId::parse("room_1").unwrap()),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn retryable_row_requires_deleting_status() {
        let row = InstanceRowV1 {
            guild_id: "7".to_string(),
            instance_id: "room_1".to_string(),
            ruleset_key: "studyroom".to_string(),
            ruleset_version: 3,
            kind: "studyroom".to_string(),
            created_by: "9".to_string(),
            status: "active".to_string(),
            resources: Json(json!({})),
        };
        assert_eq!(
            row.decode_retryable(GuildId(7)),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn zero_discord_identity_is_corruption() {
        let mut row = pinned_row("active");
        row.artifact_created_by = Some("0".to_string());
        assert!(matches!(
            row.decode(GuildId(7), &InstanceId::parse("room_1").unwrap()),
            Err(PinnedInstanceRowErrorV1::Artifact)
        ));

        let row = InstanceRowV1 {
            guild_id: "0".to_string(),
            instance_id: "room_1".to_string(),
            ruleset_key: "studyroom".to_string(),
            ruleset_version: 3,
            kind: "studyroom".to_string(),
            created_by: "9".to_string(),
            status: "active".to_string(),
            resources: Json(json!({})),
        };
        assert_eq!(
            row.decode(GuildId(0), &InstanceId::parse("room_1").unwrap()),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );

        let row = InstanceRowV1 {
            guild_id: "7".to_string(),
            instance_id: "room_1".to_string(),
            ruleset_key: "studyroom".to_string(),
            ruleset_version: 3,
            kind: "studyroom".to_string(),
            created_by: "9".to_string(),
            status: "active".to_string(),
            resources: Json(json!({"roles":{"member":"0"}})),
        };
        assert_eq!(
            row.decode(GuildId(7), &InstanceId::parse("room_1").unwrap()),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }
}
