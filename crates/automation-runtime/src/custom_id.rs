use discord_model::GuildId;

const PREFIX: &str = "starring";
const INSTANCE: &str = "i";
const MAX_CUSTOM_ID_LEN: usize = 100;
const BUTTON: &str = "button";
const MODAL: &str = "modal";
pub const PANEL_RENDER_REVISION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentKind {
    Button,
    Modal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedCustomId {
    Component {
        guild_id: GuildId,
        ruleset_key: String,
        kind: ComponentKind,
        key: String,
    },
    InstanceAction {
        instance_id: String,
        action: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomIdError {
    WrongPrefix,
    WrongShape,
    BadGuildId,
    UnknownKind,
    TooLong,
}

pub fn encode_button(guild_id: GuildId, ruleset_key: &str, button_key: &str) -> String {
    format!(
        "{PREFIX}:{}:{}:{}:{}",
        guild_id.0, ruleset_key, BUTTON, button_key
    )
}

pub fn encode_modal(guild_id: GuildId, ruleset_key: &str, modal_key: &str) -> String {
    format!(
        "{PREFIX}:{}:{}:{}:{}",
        guild_id.0, ruleset_key, MODAL, modal_key
    )
}

pub fn encode_instance_action(instance_id: &str, action: &str) -> Result<String, CustomIdError> {
    let encoded = format!("{PREFIX}:{INSTANCE}:{instance_id}:{action}");
    if encoded.len() > MAX_CUSTOM_ID_LEN {
        return Err(CustomIdError::TooLong);
    }
    Ok(encoded)
}

pub fn decode(custom_id: &str) -> Result<ParsedCustomId, CustomIdError> {
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts[0] != PREFIX {
        return Err(CustomIdError::WrongPrefix);
    }
    if parts.get(1) == Some(&INSTANCE) {
        if parts.len() != 4 {
            return Err(CustomIdError::WrongShape);
        }
        return Ok(ParsedCustomId::InstanceAction {
            instance_id: parts[2].to_string(),
            action: parts[3].to_string(),
        });
    }
    if parts.len() != 5 {
        return Err(CustomIdError::WrongShape);
    }
    let guild_id = parts[1]
        .parse::<u64>()
        .map(GuildId)
        .map_err(|_| CustomIdError::BadGuildId)?;
    let kind = match parts[3] {
        BUTTON => ComponentKind::Button,
        MODAL => ComponentKind::Modal,
        _ => return Err(CustomIdError::UnknownKind),
    };
    Ok(ParsedCustomId::Component {
        guild_id,
        ruleset_key: parts[2].to_string(),
        kind,
        key: parts[4].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_button_roundtrip() {
        let encoded = encode_button(GuildId(123), "study_demo", "create_study_button");
        assert_eq!(
            encoded,
            "starring:123:study_demo:button:create_study_button"
        );
        assert_eq!(
            decode(&encoded).unwrap(),
            ParsedCustomId::Component {
                guild_id: GuildId(123),
                ruleset_key: "study_demo".to_string(),
                kind: ComponentKind::Button,
                key: "create_study_button".to_string(),
            }
        );
    }

    #[test]
    fn encode_modal_roundtrip() {
        let encoded = encode_modal(GuildId(123), "study_demo", "create_study_modal");
        assert_eq!(encoded, "starring:123:study_demo:modal:create_study_modal");
        let parsed = decode(&encoded).unwrap();
        assert!(matches!(
            parsed,
            ParsedCustomId::Component {
                kind: ComponentKind::Modal,
                key,
                ..
            } if key == "create_study_modal"
        ));
    }

    #[test]
    fn decode_rejects_wrong_prefix() {
        assert_eq!(
            decode("nope:1:rs:button:b").unwrap_err(),
            CustomIdError::WrongPrefix
        );
    }

    #[test]
    fn decode_rejects_wrong_shape() {
        assert_eq!(
            decode("starring:1:rs:button").unwrap_err(),
            CustomIdError::WrongShape
        );
    }

    #[test]
    fn decode_rejects_bad_guild_id() {
        assert_eq!(
            decode("starring:abc:rs:button:b").unwrap_err(),
            CustomIdError::BadGuildId
        );
    }

    #[test]
    fn decode_rejects_unknown_kind() {
        assert_eq!(
            decode("starring:1:rs:select:s").unwrap_err(),
            CustomIdError::UnknownKind
        );
    }

    #[test]
    fn encode_instance_action_roundtrip() {
        let encoded = encode_instance_action("room_001", "join").unwrap();
        assert_eq!(encoded, "starring:i:room_001:join");
        assert_eq!(
            decode(&encoded).unwrap(),
            ParsedCustomId::InstanceAction {
                instance_id: "room_001".to_string(),
                action: "join".to_string(),
            }
        );
    }

    #[test]
    fn encode_instance_action_enforces_hundred_char_limit() {
        let max_instance_id = "z".repeat(32);
        let action_at_limit = "a".repeat(56);
        let encoded = encode_instance_action(&max_instance_id, &action_at_limit).unwrap();
        assert_eq!(encoded.len(), 100);
        let action_over_limit = "a".repeat(57);
        assert_eq!(
            encode_instance_action(&max_instance_id, &action_over_limit).unwrap_err(),
            CustomIdError::TooLong
        );
    }

    #[test]
    fn decode_rejects_instance_without_action() {
        assert_eq!(
            decode("starring:i:room_001").unwrap_err(),
            CustomIdError::WrongShape
        );
    }

    #[test]
    fn decode_rejects_instance_extra_segment() {
        assert_eq!(
            decode("starring:i:room_001:join:extra").unwrap_err(),
            CustomIdError::WrongShape
        );
    }

    #[test]
    fn panel_render_revision_is_one() {
        assert_eq!(PANEL_RENDER_REVISION, 1);
    }
}
