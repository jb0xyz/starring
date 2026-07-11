use std::collections::BTreeMap;
use std::env;

use automation_core::validate;
use automation_runtime::{custom_id, gateway};
use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef, InstanceKind,
    InstanceRef, InstanceResourceRefs, InteractionRule, InteractionRuleSet, ModalFieldSpec,
    ModalFieldStyle, ModalSpec, OverwriteTargetSpec, PanelSpec, RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, Permissions};
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::id::Id;

mod random_instance_id;

const RULESET_KEY: &str = "studyroom_demo";
const BUTTON_KEY: &str = "create_study_room";
const MODAL_KEY: &str = "create_study_modal";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let token = env::var("DISCORD_TEST_TOKEN")?;
    let guild_id: u64 = env::var("DISCORD_TEST_GUILD")?.parse()?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let database_url = env::var("STARRING_DATABASE_URL")?;

    let ruleset = studyroom_ruleset();
    let bindings = bindings(channel_id);
    validate(&ruleset, &bindings).expect("studyroom ruleset should validate");

    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .map_err(report_connect_error)?;
    automation_instance_postgres::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| {
            eprintln!("postgres startup: migration failed: {error}");
            "PostgreSQL startup failed during migration".to_string()
        })?;
    let instances = automation_instance_postgres::PostgresInstanceStore::new(pool);
    let instance_ids = random_instance_id::RandomInstanceIdGenerator::new();

    install_panel(&token, guild_id, channel_id).await?;
    eprintln!("postgres connected; panel installed; listening for interactions (Ctrl-C to stop)");
    gateway::run(
        token,
        RULESET_KEY.to_string(),
        ruleset,
        bindings,
        "스터디룸 생성에 실패했습니다. 봇 권한 또는 역할 순서를 확인해주세요.".to_string(),
        instances,
        instance_ids,
    )
    .await;
    Ok(())
}

fn created(key: &str) -> CreatedRef {
    CreatedRef {
        created: key.to_string(),
    }
}

fn report_connect_error(error: sqlx::Error) -> String {
    match &error {
        sqlx::Error::Configuration(_) => {
            eprintln!("postgres startup: connection failed (invalid configuration)");
        }
        other => {
            eprintln!("postgres startup: connection failed: {other}");
        }
    }
    "PostgreSQL startup failed during connection".to_string()
}

fn bindings(channel_id: u64) -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    bindings
        .channel_bindings
        .insert(ResourceKey("study_hub".to_string()), ChannelId(channel_id));
    bindings
}

fn studyroom_ruleset() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "study_panel".to_string(),
            channel: ResourceKey("study_hub".to_string()),
            content: "Create a study room".to_string(),
            buttons: vec![ButtonSpec {
                label: "Create study room".to_string(),
                route: ButtonRoute::Static {
                    key: BUTTON_KEY.to_string(),
                },
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
                    ActionSpec::DeferEphemeral,
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
                        key: "study_welcome_panel".to_string(),
                        channel: ChannelRef::Created(created("study_channel")),
                        content:
                            "스터디룸 '${input.room_name}'이 생성되었습니다. 이 채널은 스터디 멤버만 볼 수 있어요."
                                .to_string(),
                        buttons: vec![ButtonSpec {
                            label: "도움말".to_string(),
                            route: ButtonRoute::Static {
                                key: "study_help".to_string(),
                            },
                        }],
                    },
                    ActionSpec::RegisterInstance {
                        key: "study_room_instance".to_string(),
                        kind: InstanceKind("study_room".to_string()),
                        resources: InstanceResourceRefs {
                            roles: BTreeMap::from([(
                                "member_role".to_string(),
                                created("study_member_role"),
                            )]),
                            channels: BTreeMap::from([(
                                "room_channel".to_string(),
                                created("study_channel"),
                            )]),
                            messages: BTreeMap::from([(
                                "welcome_panel".to_string(),
                                created("study_welcome_panel"),
                            )]),
                        },
                    },
                    ActionSpec::PostPanel {
                        key: "study_hub_entry".to_string(),
                        channel: ChannelRef::Existing(ResourceKey("study_hub".to_string())),
                        content: "'${input.room_name}' 스터디룸이 열렸습니다.".to_string(),
                        buttons: vec![ButtonSpec {
                            label: "참가하기".to_string(),
                            route: ButtonRoute::InstanceAction {
                                instance: InstanceRef::Created(created("study_room_instance")),
                                action: "join".to_string(),
                            },
                        }],
                    },
                    ActionSpec::EditResponse {
                        content: "스터디룸 '${input.room_name}' 생성 완료! 새 채널을 확인하세요."
                            .to_string(),
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
                        "이 채널은 스터디 멤버만 볼 수 있는 비공개 스터디룸입니다."
                            .to_string(),
                }],
            },
            InteractionRule {
                key: "study_join_rule".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "join".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::GrantRole {
                        role: RoleRef::Instance {
                            instance: InstanceRef::Event,
                            alias: "member_role".to_string(),
                        },
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::EditResponse {
                        content: "스터디룸에 참가했습니다.".to_string(),
                    },
                ],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studyroom_ruleset_validates() {
        validate(&studyroom_ruleset(), &bindings(1)).expect("studyroom ruleset should validate");
    }

    #[test]
    fn join_rule_uses_deferred_contract() {
        let ruleset = studyroom_ruleset();
        let join = ruleset
            .rules
            .iter()
            .find(|rule| rule.key == "study_join_rule")
            .expect("join rule present");
        assert!(matches!(
            join.actions.first(),
            Some(ActionSpec::DeferEphemeral)
        ));
        assert!(matches!(
            join.actions.last(),
            Some(ActionSpec::EditResponse { .. })
        ));
    }
}
