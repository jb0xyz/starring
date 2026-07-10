use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;
use discord_model::Permissions;

use crate::modal::ModalSpec;
use crate::panel::PanelSpec;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionRuleSet {
    pub version: u32,
    #[serde(default)]
    pub panels: Vec<PanelSpec>,
    #[serde(default)]
    pub modals: Vec<ModalSpec>,
    #[serde(default)]
    pub rules: Vec<InteractionRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionRule {
    pub key: String,
    pub trigger: TriggerSpec,
    pub actions: Vec<ActionSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerSpec {
    ButtonClick { component: String },
    ModalSubmit { modal: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionSpec {
    GrantRole {
        role: RoleRef,
        target: ActionTarget,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal {
        modal: String,
    },
    CreateChannel {
        key: String,
        name: String,
    },
    CreateRole {
        key: String,
        name: String,
    },
    UpsertOverwrite {
        channel: ChannelRef,
        target: OverwriteTargetSpec,
        #[serde(default)]
        allow: Permissions,
        #[serde(default)]
        deny: Permissions,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatedRef {
    pub created: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoleRef {
    Existing(ResourceKey),
    Created(CreatedRef),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChannelRef {
    Existing(ResourceKey),
    Created(CreatedRef),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverwriteTargetSpec {
    Everyone,
    Role(RoleRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionTarget {
    Actor,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{ButtonSpec, PanelSpec};
    use desired_state::ResourceKey;
    use discord_model::Permissions;

    fn sample() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![PanelSpec {
                key: "verify_panel".to_string(),
                channel: ResourceKey("verify_channel".to_string()),
                content: "click to verify".to_string(),
                buttons: vec![ButtonSpec {
                    key: "verify_button".to_string(),
                    label: "Verify".to_string(),
                }],
            }],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "verify_rule".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "verify_button".to_string(),
                },
                actions: vec![
                    ActionSpec::GrantRole {
                        role: RoleRef::Existing(ResourceKey("verified_member".to_string())),
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::RespondEphemeral {
                        content: "welcome".to_string(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn ruleset_json_roundtrips() {
        let set = sample();
        let json = serde_json::to_string(&set).unwrap();
        let back: InteractionRuleSet = serde_json::from_str(&json).unwrap();
        assert_eq!(set, back);
    }

    #[test]
    fn trigger_and_action_tagged_shape() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains(r#""type":"button_click""#));
        assert!(json.contains(r#""type":"grant_role""#));
        assert!(json.contains(r#""type":"respond_ephemeral""#));
        assert!(json.contains(r#""target":"actor""#));
    }

    #[test]
    fn panels_and_rules_default_when_absent() {
        let set: InteractionRuleSet = serde_json::from_str(r#"{"version":2}"#).unwrap();
        assert_eq!(set.version, 2);
        assert!(set.panels.is_empty());
        assert!(set.modals.is_empty());
        assert!(set.rules.is_empty());
    }

    #[test]
    fn conditions_field_is_rejected() {
        let json = r#"{"key":"r","trigger":{"type":"button_click","component":"b"},"conditions":[],"actions":[]}"#;
        assert!(serde_json::from_str::<InteractionRule>(json).is_err());
    }

    #[test]
    fn unknown_action_type_is_rejected() {
        let json = r#"{"type":"post_panel","channel":"x"}"#;
        assert!(serde_json::from_str::<ActionSpec>(json).is_err());
    }

    #[test]
    fn create_actions_roundtrip() {
        let channel_json =
            r#"{"type":"create_channel","key":"ch","name":"study-${input.room_name}"}"#;
        let role_json = r#"{"type":"create_role","key":"role","name":"${input.room_name} member"}"#;
        assert_eq!(
            serde_json::from_str::<ActionSpec>(channel_json).unwrap(),
            ActionSpec::CreateChannel {
                key: "ch".to_string(),
                name: "study-${input.room_name}".to_string(),
            }
        );
        assert_eq!(
            serde_json::from_str::<ActionSpec>(role_json).unwrap(),
            ActionSpec::CreateRole {
                key: "role".to_string(),
                name: "${input.room_name} member".to_string(),
            }
        );
    }

    #[test]
    fn unknown_field_in_action_is_rejected() {
        let json = r#"{"type":"grant_role","role":"verified","target":"actor","template":"x"}"#;
        assert!(serde_json::from_str::<ActionSpec>(json).is_err());
    }

    #[test]
    fn channel_ref_serde_shapes() {
        assert_eq!(
            serde_json::from_str::<ChannelRef>(r#""general""#).unwrap(),
            ChannelRef::Existing(ResourceKey("general".to_string()))
        );
        let created = ChannelRef::Created(CreatedRef {
            created: "study_channel".to_string(),
        });
        assert_eq!(
            serde_json::to_string(&created).unwrap(),
            r#"{"created":"study_channel"}"#
        );
        assert_eq!(
            serde_json::from_str::<ChannelRef>(r#"{"created":"study_channel"}"#).unwrap(),
            created
        );
    }

    #[test]
    fn overwrite_target_serde_shapes() {
        assert_eq!(
            serde_json::to_string(&OverwriteTargetSpec::Everyone).unwrap(),
            r#""everyone""#
        );
        assert_eq!(
            serde_json::from_str::<OverwriteTargetSpec>(r#""everyone""#).unwrap(),
            OverwriteTargetSpec::Everyone
        );
        let existing = OverwriteTargetSpec::Role(RoleRef::Existing(ResourceKey(
            "verified_member".to_string(),
        )));
        assert_eq!(
            serde_json::to_string(&existing).unwrap(),
            r#"{"role":"verified_member"}"#
        );
        let created = OverwriteTargetSpec::Role(RoleRef::Created(CreatedRef {
            created: "study_member_role".to_string(),
        }));
        assert_eq!(
            serde_json::to_string(&created).unwrap(),
            r#"{"role":{"created":"study_member_role"}}"#
        );
        assert_eq!(
            serde_json::from_str::<OverwriteTargetSpec>(
                r#"{"role":{"created":"study_member_role"}}"#
            )
            .unwrap(),
            created
        );
    }

    #[test]
    fn created_ref_rejects_unknown_field() {
        assert!(serde_json::from_str::<ChannelRef>(r#"{"created":"x","extra":"y"}"#).is_err());
        assert!(serde_json::from_str::<RoleRef>(r#"{"created":"x","extra":"y"}"#).is_err());
    }

    #[test]
    fn upsert_overwrite_action_roundtrips() {
        let json = r#"{"type":"upsert_overwrite","channel":{"created":"study_channel"},"target":"everyone","allow":"0","deny":"1024"}"#;
        let action: ActionSpec = serde_json::from_str(json).unwrap();
        assert_eq!(
            action,
            ActionSpec::UpsertOverwrite {
                channel: ChannelRef::Created(CreatedRef {
                    created: "study_channel".to_string(),
                }),
                target: OverwriteTargetSpec::Everyone,
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            }
        );
        let unknown = r#"{"type":"upsert_overwrite","channel":"g","target":"everyone","evil":1}"#;
        assert!(serde_json::from_str::<ActionSpec>(unknown).is_err());
    }
}
