use std::collections::BTreeMap;

use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    ResourceState, RoleIntent,
};
use discord_model::{ChannelType, Permissions};

use crate::generate::GenerateInput;

pub fn build_system_prompt() -> String {
    let mut prompt = String::from(SCHEMA_GUIDE);
    prompt.push_str("\n\nExamples of valid DesiredState JSON:\n");
    for desired_state in example_desired_states() {
        prompt.push_str(&serde_json::to_string(&desired_state).unwrap());
        prompt.push('\n');
    }
    prompt.push_str(
        "\nOutput ONLY the JSON document. No markdown fences, no explanation, no comments.",
    );
    prompt
}

pub fn build_user_prompt(input: &GenerateInput) -> String {
    format!(
        "Current server:\n{}\n\nRequest:\n{}",
        input.guild_context_summary, input.user_prompt
    )
}

fn example_desired_states() -> Vec<DesiredState> {
    let role = |key: &str, name: &str| RoleIntent {
        identity: Identity {
            key: ResourceKey(key.to_string()),
            ..Default::default()
        },
        name: Some(name.to_string()),
        permissions: Some(Permissions::empty()),
    };

    let vip = DesiredState {
        roles: vec![role("vip", "VIP")],
        ..Default::default()
    };

    let mut roles = BTreeMap::new();
    roles.insert(
        ResourceKey("verified".to_string()),
        AccessGrant {
            allow: vec![Capability::View, Capability::Send],
            deny: vec![],
        },
    );
    let auth = DesiredState {
        roles: vec![role("verified", "Verified")],
        channels: vec![ChannelIntent {
            identity: Identity {
                key: ResourceKey("general".to_string()),
                ..Default::default()
            },
            name: Some("general".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: Some(AccessGrant {
                    allow: vec![],
                    deny: vec![Capability::View],
                }),
                roles,
            }),
            raw_overwrites: None,
        }],
        ..Default::default()
    };

    let delete = DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: ResourceKey("vip".to_string()),
                state: ResourceState::Absent,
                ..Default::default()
            },
            name: Some("VIP".to_string()),
            permissions: None,
        }],
        ..Default::default()
    };

    vec![vip, auth, delete]
}

const SCHEMA_GUIDE: &str = "You output a DesiredState JSON for a Discord server.\nTop-level: \"mode\" (use \"patch\"), \"roles\" [], \"channels\" [], \"features\" [].\nRole: {\"key\":\"<logical id>\",\"name\":\"<name>\",\"permissions\":\"0\",\"match\":{\"by\":\"by_name\"},\"ownership\":\"managed\",\"state\":\"present\"}. To delete set \"state\":\"absent\".\nChannel: {\"key\":\"...\",\"name\":\"...\",\"channel_type\":\"text\",\"access\":{\"everyone\":{\"allow\":[],\"deny\":[\"view\"]},\"roles\":{\"<role key>\":{\"allow\":[\"view\",\"send\"],\"deny\":[]}}}}.\nCapabilities: view, send, read_history, add_reactions, attach_files, embed_links, manage_messages, connect, speak.\nReference roles by their key. Never grant administrator.";
