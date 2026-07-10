use std::env;

use automation_runtime::{custom_id, gateway};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, RoleId};
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::id::Id;

const RULESET_KEY: &str = "demo_verify";
const BUTTON_KEY: &str = "verify_button";
const ROLE_KEY: &str = "verified_member";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let token = env::var("DISCORD_TEST_TOKEN")?;
    let guild_id: u64 = env::var("DISCORD_TEST_GUILD")?.parse()?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let role_id: u64 = env::var("DISCORD_TEST_ROLE")?.parse()?;

    let ruleset = demo_ruleset();
    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey(ROLE_KEY.to_string()), RoleId(role_id));

    install_panel(&token, guild_id, channel_id).await?;
    eprintln!("panel installed; listening for button clicks (Ctrl-C to stop)");
    gateway::run(token, ruleset, bindings).await;
    Ok(())
}

fn demo_ruleset() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "demo_panel".to_string(),
            channel: ResourceKey("verify_channel".to_string()),
            content: "Click to verify".to_string(),
            buttons: vec![ButtonSpec {
                key: BUTTON_KEY.to_string(),
                label: "Verify".to_string(),
            }],
        }],
        rules: vec![InteractionRule {
            key: "demo_verify_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: BUTTON_KEY.to_string(),
            },
            actions: vec![
                ActionSpec::GrantRole {
                    role: ResourceKey(ROLE_KEY.to_string()),
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: "You are verified!".to_string(),
                },
            ],
        }],
    }
}

async fn install_panel(
    token: &str,
    guild_id: u64,
    channel_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let http = Client::new(token.to_string());
    let encoded = custom_id::encode(GuildId(guild_id), RULESET_KEY, BUTTON_KEY);
    let button = Component::Button(Button {
        id: None,
        custom_id: Some(encoded),
        disabled: false,
        emoji: None,
        label: Some("Verify".to_string()),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    });
    let components = [Component::ActionRow(ActionRow {
        id: None,
        components: vec![button],
    })];
    http.create_message(Id::new(channel_id))
        .content("Verification panel")
        .components(&components)
        .await?;
    Ok(())
}
