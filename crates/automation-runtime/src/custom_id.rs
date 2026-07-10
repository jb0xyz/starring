use discord_model::GuildId;

const PREFIX: &str = "starring";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCustomId {
    pub guild_id: GuildId,
    pub ruleset_key: String,
    pub button_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomIdError {
    WrongPrefix,
    WrongShape,
    BadGuildId,
}

pub fn encode(guild_id: GuildId, ruleset_key: &str, button_key: &str) -> String {
    format!("{PREFIX}:{}:{}:{}", guild_id.0, ruleset_key, button_key)
}

pub fn decode(custom_id: &str) -> Result<ParsedCustomId, CustomIdError> {
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts.len() != 4 {
        return Err(CustomIdError::WrongShape);
    }
    if parts[0] != PREFIX {
        return Err(CustomIdError::WrongPrefix);
    }
    let guild_id = parts[1]
        .parse::<u64>()
        .map(GuildId)
        .map_err(|_| CustomIdError::BadGuildId)?;
    Ok(ParsedCustomId {
        guild_id,
        ruleset_key: parts[2].to_string(),
        button_key: parts[3].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::GuildId;

    #[test]
    fn encode_decode_roundtrip() {
        let encoded = encode(GuildId(123456789), "demo_verify", "verify_button");
        assert_eq!(encoded, "starring:123456789:demo_verify:verify_button");
        assert_eq!(
            decode(&encoded).unwrap(),
            ParsedCustomId {
                guild_id: GuildId(123456789),
                ruleset_key: "demo_verify".to_string(),
                button_key: "verify_button".to_string(),
            }
        );
    }

    #[test]
    fn decode_rejects_wrong_prefix() {
        assert_eq!(
            decode("nope:1:rs:btn").unwrap_err(),
            CustomIdError::WrongPrefix
        );
    }

    #[test]
    fn decode_rejects_wrong_shape() {
        assert_eq!(
            decode("starring:1:rs").unwrap_err(),
            CustomIdError::WrongShape
        );
    }

    #[test]
    fn decode_rejects_bad_guild_id() {
        assert_eq!(
            decode("starring:abc:rs:btn").unwrap_err(),
            CustomIdError::BadGuildId
        );
    }
}
