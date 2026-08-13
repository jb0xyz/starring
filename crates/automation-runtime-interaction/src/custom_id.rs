use discord_model::GuildId;

const PREFIX: &str = "starring";
const INSTANCE: &str = "i";
pub const MAX_INTERACTION_CUSTOM_ID_BYTES_V1: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionComponentKindV1 {
    Button,
    Modal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedInteractionCustomIdV1 {
    Component {
        guild_id: GuildId,
        ruleset_key: String,
        kind: InteractionComponentKindV1,
        key: String,
    },
    InstanceAction {
        instance_id: String,
        action: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionCustomIdErrorV1 {
    #[error("interaction custom identifier has the wrong prefix")]
    WrongPrefix,
    #[error("interaction custom identifier has the wrong shape")]
    WrongShape,
    #[error("interaction custom identifier has an invalid guild")]
    BadGuildId,
    #[error("interaction custom identifier has an unknown component kind")]
    UnknownKind,
    #[error("interaction custom identifier exceeds Discord's byte limit")]
    TooLong,
}

pub fn encode_interaction_button_custom_id_v1(
    guild_id: GuildId,
    ruleset_key: &str,
    button_key: &str,
) -> String {
    format!("{PREFIX}:{}:{ruleset_key}:button:{button_key}", guild_id.0)
}

pub fn encode_interaction_modal_custom_id_v1(
    guild_id: GuildId,
    ruleset_key: &str,
    modal_key: &str,
) -> String {
    format!("{PREFIX}:{}:{ruleset_key}:modal:{modal_key}", guild_id.0)
}

pub fn encode_interaction_instance_action_custom_id_v1(
    instance_id: &str,
    action: &str,
) -> Result<String, InteractionCustomIdErrorV1> {
    let encoded = format!("{PREFIX}:{INSTANCE}:{instance_id}:{action}");
    if encoded.len() > MAX_INTERACTION_CUSTOM_ID_BYTES_V1 {
        return Err(InteractionCustomIdErrorV1::TooLong);
    }
    Ok(encoded)
}

pub fn decode_interaction_custom_id_v1(
    custom_id: &str,
) -> Result<ParsedInteractionCustomIdV1, InteractionCustomIdErrorV1> {
    if custom_id.len() > MAX_INTERACTION_CUSTOM_ID_BYTES_V1 {
        return Err(InteractionCustomIdErrorV1::TooLong);
    }
    let parts = custom_id.split(':').collect::<Vec<_>>();
    if parts.first() != Some(&PREFIX) {
        return Err(InteractionCustomIdErrorV1::WrongPrefix);
    }
    if parts.get(1) == Some(&INSTANCE) {
        if parts.len() != 4 {
            return Err(InteractionCustomIdErrorV1::WrongShape);
        }
        return Ok(ParsedInteractionCustomIdV1::InstanceAction {
            instance_id: parts[2].to_string(),
            action: parts[3].to_string(),
        });
    }
    if parts.len() != 5 {
        return Err(InteractionCustomIdErrorV1::WrongShape);
    }
    let guild_id = parts[1]
        .parse::<u64>()
        .map(GuildId)
        .map_err(|_| InteractionCustomIdErrorV1::BadGuildId)?;
    let kind = match parts[3] {
        "button" => InteractionComponentKindV1::Button,
        "modal" => InteractionComponentKindV1::Modal,
        _ => return Err(InteractionCustomIdErrorV1::UnknownKind),
    };
    Ok(ParsedInteractionCustomIdV1::Component {
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
    fn every_custom_identifier_kind_roundtrips() {
        let button = encode_interaction_button_custom_id_v1(GuildId(123), "study", "join");
        assert_eq!(
            decode_interaction_custom_id_v1(&button).unwrap(),
            ParsedInteractionCustomIdV1::Component {
                guild_id: GuildId(123),
                ruleset_key: "study".to_string(),
                kind: InteractionComponentKindV1::Button,
                key: "join".to_string(),
            }
        );
        let modal = encode_interaction_modal_custom_id_v1(GuildId(123), "study", "create");
        assert!(matches!(
            decode_interaction_custom_id_v1(&modal),
            Ok(ParsedInteractionCustomIdV1::Component {
                kind: InteractionComponentKindV1::Modal,
                ..
            })
        ));
        let instance = encode_interaction_instance_action_custom_id_v1("room_1", "close").unwrap();
        assert_eq!(
            decode_interaction_custom_id_v1(&instance).unwrap(),
            ParsedInteractionCustomIdV1::InstanceAction {
                instance_id: "room_1".to_string(),
                action: "close".to_string(),
            }
        );
    }

    #[test]
    fn instance_action_enforces_the_exact_hundred_byte_limit() {
        let instance_id = "z".repeat(32);
        assert_eq!(
            encode_interaction_instance_action_custom_id_v1(&instance_id, &"a".repeat(56))
                .unwrap()
                .len(),
            MAX_INTERACTION_CUSTOM_ID_BYTES_V1
        );
        assert_eq!(
            encode_interaction_instance_action_custom_id_v1(&instance_id, &"a".repeat(57)),
            Err(InteractionCustomIdErrorV1::TooLong)
        );
    }

    #[test]
    fn invalid_wire_shapes_remain_fail_closed() {
        assert_eq!(
            decode_interaction_custom_id_v1("nope:1:rs:button:b"),
            Err(InteractionCustomIdErrorV1::WrongPrefix)
        );
        assert_eq!(
            decode_interaction_custom_id_v1("starring:1:rs:button"),
            Err(InteractionCustomIdErrorV1::WrongShape)
        );
        assert_eq!(
            decode_interaction_custom_id_v1("starring:abc:rs:button:b"),
            Err(InteractionCustomIdErrorV1::BadGuildId)
        );
        assert_eq!(
            decode_interaction_custom_id_v1("starring:1:rs:select:s"),
            Err(InteractionCustomIdErrorV1::UnknownKind)
        );
        assert_eq!(
            decode_interaction_custom_id_v1("starring:i:room_1"),
            Err(InteractionCustomIdErrorV1::WrongShape)
        );
        assert_eq!(
            decode_interaction_custom_id_v1(&format!("starring:1:rs:button:{}", "x".repeat(80))),
            Err(InteractionCustomIdErrorV1::TooLong)
        );
    }
}
