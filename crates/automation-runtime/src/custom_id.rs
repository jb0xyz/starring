use discord_model::GuildId;

const PREFIX: &str = "starring";
const BUTTON: &str = "button";
const MODAL: &str = "modal";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentKind {
    Button,
    Modal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCustomId {
    pub guild_id: GuildId,
    pub ruleset_key: String,
    pub kind: ComponentKind,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomIdError {
    WrongPrefix,
    WrongShape,
    BadGuildId,
    UnknownKind,
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

pub fn decode(custom_id: &str) -> Result<ParsedCustomId, CustomIdError> {
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts.len() != 5 {
        return Err(CustomIdError::WrongShape);
    }
    if parts[0] != PREFIX {
        return Err(CustomIdError::WrongPrefix);
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
    Ok(ParsedCustomId {
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
            ParsedCustomId {
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
        assert_eq!(parsed.kind, ComponentKind::Modal);
        assert_eq!(parsed.key, "create_study_modal");
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
}
