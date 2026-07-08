use desired_state::Capability;
use discord_model::Permissions;

pub fn capability_to_permission(cap: Capability) -> Permissions {
    match cap {
        Capability::View => Permissions::VIEW_CHANNEL,
        Capability::Send => Permissions::SEND_MESSAGES,
        Capability::ReadHistory => Permissions::READ_MESSAGE_HISTORY,
        Capability::AddReactions => Permissions::ADD_REACTIONS,
        Capability::AttachFiles => Permissions::ATTACH_FILES,
        Capability::EmbedLinks => Permissions::EMBED_LINKS,
        Capability::ManageMessages => Permissions::MANAGE_MESSAGES,
        Capability::Connect => Permissions::CONNECT,
        Capability::Speak => Permissions::SPEAK,
    }
}

pub fn capabilities_to_permissions(caps: &[Capability]) -> Permissions {
    caps.iter().fold(Permissions::empty(), |acc, &cap| {
        acc | capability_to_permission(cap)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::Capability;
    use discord_model::Permissions;

    #[test]
    fn maps_each_capability() {
        assert_eq!(
            capability_to_permission(Capability::View),
            Permissions::VIEW_CHANNEL
        );
        assert_eq!(
            capability_to_permission(Capability::Send),
            Permissions::SEND_MESSAGES
        );
        assert_eq!(
            capability_to_permission(Capability::ReadHistory),
            Permissions::READ_MESSAGE_HISTORY
        );
        assert_eq!(
            capability_to_permission(Capability::Speak),
            Permissions::SPEAK
        );
    }

    #[test]
    fn unions_capabilities() {
        let p = capabilities_to_permissions(&[Capability::View, Capability::Send]);
        assert_eq!(p, Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES);
    }
}
