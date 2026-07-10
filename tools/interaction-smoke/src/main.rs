use std::env;

use automation_core::validate;
use automation_runtime::{custom_id, gateway};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, ChannelRef, CreatedRef, InteractionRule,
    InteractionRuleSet, ModalFieldSpec, ModalFieldStyle, ModalSpec, OverwriteTargetSpec, PanelSpec,
    RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions};
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::id::Id;

const RULESET_KEY: &str = "studyroom_demo";
const BUTTON_KEY: &str = "create_study_room";
const MODAL_KEY: &str = "create_study_modal";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let token = env::var("DISCORD_TEST_TOKEN")?;
    let guild_id: u64 = env::var("DISCORD_TEST_GUILD")?.parse()?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;

    let ruleset = studyroom_ruleset();
    validate(&ruleset, &ResourceBindingMap::default()).expect("studyroom ruleset should validate");

    install_panel(&token, guild_id, channel_id).await?;
    eprintln!("panel installed; listening for interactions (Ctrl-C to stop)");
    gateway::run(
        token,
        RULESET_KEY.to_string(),
        ruleset,
        ResourceBindingMap::default(),
    )
    .await;
    Ok(())
}

fn created(key: &str) -> CreatedRef {
    CreatedRef {
        created: key.to_string(),
    }
}

fn studyroom_ruleset() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "study_panel".to_string(),
            channel: ResourceKey("study_channel".to_string()),
            content: "Create a study room".to_string(),
            buttons: vec![ButtonSpec {
                key: BUTTON_KEY.to_string(),
                label: "Create study room".to_string(),
            }],
        }],
        modals: vec![ModalSpec {
            key: MODAL_KEY.to_string(),
            title: "Create study room".to_string(),
            fields: vec![ModalFieldSpec {
                key: "room_name".to_string(),
                label: "Room name".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
            }],
        }],
        rules: vec![
            InteractionRule {
                key: "open_study_modal".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: BUTTON_KEY.to_string(),
                },
                actions: vec![ActionSpec::OpenModal {
                    modal: MODAL_KEY.to_string(),
                }],
            },
            InteractionRule {
                key: "submit_study_modal".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: MODAL_KEY.to_string(),
                },
                actions: vec![
                    ActionSpec::RespondEphemeral {
                        content:
                            "스터디룸 '${input.room_name}'을 만들고 있어요. 곧 새 채널이 나타납니다."
                                .to_string(),
                    },
                    ActionSpec::CreateRole {
                        key: "study_member_role".to_string(),
                        name: "${input.room_name} 멤버".to_string(),
                    },
                    ActionSpec::CreateChannel {
                        key: "study_channel".to_string(),
                        name: "study-${input.room_name}".to_string(),
                    },
                    ActionSpec::UpsertOverwrite {
                        channel: ChannelRef::Created(created("study_channel")),
                        target: OverwriteTargetSpec::Everyone,
                        allow: Permissions::empty(),
                        deny: Permissions::VIEW_CHANNEL,
                    },
                    ActionSpec::UpsertOverwrite {
                        channel: ChannelRef::Created(created("study_channel")),
                        target: OverwriteTargetSpec::Role(RoleRef::Created(created(
                            "study_member_role",
                        ))),
                        allow: Permissions::VIEW_CHANNEL,
                        deny: Permissions::empty(),
                    },
                    ActionSpec::GrantRole {
                        role: RoleRef::Created(created("study_member_role")),
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::PostPanel {
                        channel: ChannelRef::Created(created("study_channel")),
                        content:
                            "스터디룸 '${input.room_name}'이 생성되었습니다. 이 채널은 스터디 멤버만 볼 수 있어요."
                                .to_string(),
                        buttons: vec![ButtonSpec {
                            key: "study_help".to_string(),
                            label: "도움말".to_string(),
                        }],
                    },
                ],
            },
            InteractionRule {
                key: "study_help_rule".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "study_help".to_string(),
                },
                actions: vec![ActionSpec::RespondEphemeral {
                    content:
                        "이 채널은 스터디 멤버만 볼 수 있는 비공개 스터디룸입니다. 공개 참가 기능은 다음 단계에서 연결됩니다."
                            .to_string(),
                }],
            },
        ],
    }
}

async fn install_panel(
    token: &str,
    guild_id: u64,
    channel_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let http = Client::new(token.to_string());
    let encoded = custom_id::encode_button(GuildId(guild_id), RULESET_KEY, BUTTON_KEY);
    let button = Component::Button(Button {
        id: None,
        custom_id: Some(encoded),
        disabled: false,
        emoji: None,
        label: Some("Create study room".to_string()),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    });
    let components = [Component::ActionRow(ActionRow {
        id: None,
        components: vec![button],
    })];
    http.create_message(Id::new(channel_id))
        .content("Study room panel")
        .components(&components)
        .await?;
    Ok(())
}
