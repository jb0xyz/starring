pub use automation_runtime_interaction::{
    decode_interaction_custom_id_v1 as decode,
    encode_interaction_button_custom_id_v1 as encode_button,
    encode_interaction_instance_action_custom_id_v1 as encode_instance_action,
    encode_interaction_modal_custom_id_v1 as encode_modal,
    InteractionComponentKindV1 as ComponentKind, InteractionCustomIdErrorV1 as CustomIdError,
    ParsedInteractionCustomIdV1 as ParsedCustomId,
};

pub const PANEL_RENDER_REVISION: u32 = 1;

#[cfg(test)]
mod tests {
    use discord_model::GuildId;

    use super::*;

    #[test]
    fn shared_codec_preserves_all_runtime_wire_encodings() {
        let button = encode_button(GuildId(123), "study_demo", "create_study_button");
        assert_eq!(button, "starring:123:study_demo:button:create_study_button");
        assert!(matches!(
            decode(&button),
            Ok(ParsedCustomId::Component {
                kind: ComponentKind::Button,
                ..
            })
        ));

        let modal = encode_modal(GuildId(123), "study_demo", "create_study_modal");
        assert_eq!(modal, "starring:123:study_demo:modal:create_study_modal");
        assert!(matches!(
            decode(&modal),
            Ok(ParsedCustomId::Component {
                kind: ComponentKind::Modal,
                ..
            })
        ));

        let instance = encode_instance_action("room_001", "join").unwrap();
        assert_eq!(instance, "starring:i:room_001:join");
        assert!(matches!(
            decode(&instance),
            Ok(ParsedCustomId::InstanceAction { .. })
        ));
        assert_eq!(PANEL_RENDER_REVISION, 1);
    }
}
