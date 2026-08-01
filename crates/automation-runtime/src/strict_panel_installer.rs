use automation_panel_installation::{
    strict::{
        StrictDeclaredPanelV1, StrictDeleteOutcomeV1, StrictExternalPostResultV1,
        StrictObservedMessageV1, StrictPanelActionRowPayloadV1, StrictPanelButtonPayloadV1,
        StrictPanelInstaller, StrictPanelMessagePayloadV1,
    },
    InstallerError,
};
use automation_state::{ButtonRoute, PanelSpec};
use discord_model::{ChannelId, GuildId, MessageId};
use twilight_http::{api_error::ApiError, error::ErrorType, Client};
use twilight_model::{
    channel::message::component::{ActionRow, Button, ButtonStyle, Component},
    id::Id,
};

use crate::custom_id::{decode, encode_button, ComponentKind, ParsedCustomId};

const MAX_CONTENT_CHARS: usize = 2_000;
const MAX_BUTTONS: usize = 5;
const MAX_BUTTON_LABEL_CHARS: usize = 80;
const MAX_CUSTOM_ID_BYTES: usize = 100;
const UNKNOWN_CHANNEL: u64 = 10_003;
const UNKNOWN_MESSAGE: u64 = 10_008;

pub struct TwilightStrictPanelInstaller<'a> {
    http: &'a Client,
}

impl<'a> TwilightStrictPanelInstaller<'a> {
    pub fn new(http: &'a Client) -> Self {
        Self { http }
    }
}

pub fn render_strict_declared_panel_v1(
    guild_id: GuildId,
    ruleset_key: &str,
    spec: &PanelSpec,
) -> Result<StrictDeclaredPanelV1, InstallerError> {
    if guild_id.0 == 0
        || ruleset_key.is_empty()
        || ruleset_key.len() > 64
        || !ruleset_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || (spec.content.is_empty() && spec.buttons.is_empty())
        || spec.content.chars().count() > MAX_CONTENT_CHARS
        || spec.buttons.len() > MAX_BUTTONS
    {
        return Err(InstallerError::new("strict panel shape is invalid"));
    }
    let buttons = spec
        .buttons
        .iter()
        .map(|button| {
            if button.label.is_empty() || button.label.chars().count() > MAX_BUTTON_LABEL_CHARS {
                return Err(InstallerError::new("strict panel button label is invalid"));
            }
            let ButtonRoute::Static { key } = &button.route else {
                return Err(InstallerError::new(
                    "strict panels require static button routes",
                ));
            };
            if key.is_empty() {
                return Err(InstallerError::new("strict panel button key is invalid"));
            }
            let custom_id = encode_button(guild_id, ruleset_key, key);
            if custom_id.len() > MAX_CUSTOM_ID_BYTES
                || !matches!(
                    decode(&custom_id),
                    Ok(ParsedCustomId::Component {
                        guild_id: decoded_guild,
                        ruleset_key: decoded_ruleset,
                        kind: ComponentKind::Button,
                        key: decoded_key,
                    }) if decoded_guild == guild_id
                        && decoded_ruleset.as_str() == ruleset_key
                        && decoded_key.as_str() == key.as_str()
                )
            {
                return Err(InstallerError::new("strict panel custom id is invalid"));
            }
            Ok(StrictPanelButtonPayloadV1 {
                label: button.label.clone(),
                custom_id,
                style: "primary".to_string(),
                disabled: false,
            })
        })
        .collect::<Result<Vec<_>, InstallerError>>()?;
    let action_rows = if buttons.is_empty() {
        Vec::new()
    } else {
        vec![StrictPanelActionRowPayloadV1 { buttons }]
    };
    Ok(StrictDeclaredPanelV1 {
        spec: spec.clone(),
        expected_payload: StrictPanelMessagePayloadV1 {
            content: spec.content.clone(),
            action_rows,
        },
    })
}

fn build_components(
    payload: &StrictPanelMessagePayloadV1,
) -> Result<Vec<Component>, InstallerError> {
    payload
        .action_rows
        .iter()
        .map(|row| {
            let buttons = row
                .buttons
                .iter()
                .map(|button| {
                    if button.style != "primary" {
                        return Err(InstallerError::new("strict panel button style is invalid"));
                    }
                    Ok(Component::Button(Button {
                        id: None,
                        custom_id: Some(button.custom_id.clone()),
                        disabled: button.disabled,
                        emoji: None,
                        label: Some(button.label.clone()),
                        style: ButtonStyle::Primary,
                        url: None,
                        sku_id: None,
                    }))
                })
                .collect::<Result<Vec<_>, InstallerError>>()?;
            Ok(Component::ActionRow(ActionRow {
                id: None,
                components: buttons,
            }))
        })
        .collect()
}

fn observed_payload(content: &str, components: &[Component]) -> StrictPanelMessagePayloadV1 {
    let action_rows = components
        .iter()
        .map(|component| {
            let Component::ActionRow(row) = component else {
                return unsupported_row();
            };
            let buttons = row
                .components
                .iter()
                .map(|component| match component {
                    Component::Button(button)
                        if button.emoji.is_none()
                            && button.url.is_none()
                            && button.sku_id.is_none() =>
                    {
                        StrictPanelButtonPayloadV1 {
                            label: button.label.clone().unwrap_or_default(),
                            custom_id: button.custom_id.clone().unwrap_or_default(),
                            style: button_style(button.style),
                            disabled: button.disabled,
                        }
                    }
                    _ => unsupported_button(),
                })
                .collect();
            StrictPanelActionRowPayloadV1 { buttons }
        })
        .collect();
    StrictPanelMessagePayloadV1 {
        content: content.to_string(),
        action_rows,
    }
}

fn unsupported_row() -> StrictPanelActionRowPayloadV1 {
    StrictPanelActionRowPayloadV1 {
        buttons: vec![unsupported_button()],
    }
}

fn unsupported_button() -> StrictPanelButtonPayloadV1 {
    StrictPanelButtonPayloadV1 {
        label: String::new(),
        custom_id: String::new(),
        style: "unsupported".to_string(),
        disabled: true,
    }
}

fn button_style(style: ButtonStyle) -> String {
    match style {
        ButtonStyle::Primary => "primary".to_string(),
        ButtonStyle::Secondary => "secondary".to_string(),
        ButtonStyle::Success => "success".to_string(),
        ButtonStyle::Danger => "danger".to_string(),
        ButtonStyle::Link => "link".to_string(),
        ButtonStyle::Premium => "premium".to_string(),
        ButtonStyle::Unknown(value) => format!("unknown_{value}"),
        _ => "unsupported".to_string(),
    }
}

fn response_code(error: &ApiError) -> Option<u64> {
    match error {
        ApiError::General(error) => Some(error.code),
        _ => None,
    }
}

fn observe_missing(kind: &ErrorType) -> bool {
    matches!(
        kind,
        ErrorType::Response { error, status, .. }
            if status.get() == 404
                && matches!(response_code(error), Some(UNKNOWN_CHANNEL | UNKNOWN_MESSAGE))
    )
}

fn post_response_failure(status: u16) -> StrictExternalPostResultV1 {
    if (400..500).contains(&status) {
        StrictExternalPostResultV1::DefinitelyNotApplied
    } else {
        StrictExternalPostResultV1::Ambiguous
    }
}

fn post_failure(kind: &ErrorType) -> StrictExternalPostResultV1 {
    match kind {
        ErrorType::BuildingRequest
        | ErrorType::CreatingHeader { .. }
        | ErrorType::Json
        | ErrorType::Unauthorized
        | ErrorType::Validation => StrictExternalPostResultV1::DefinitelyNotApplied,
        ErrorType::Response { status, .. } => post_response_failure(status.get()),
        _ => StrictExternalPostResultV1::Ambiguous,
    }
}

fn delete_response_failure(status: u16, code: Option<u64>) -> StrictDeleteOutcomeV1 {
    if status == 404 && matches!(code, Some(UNKNOWN_CHANNEL | UNKNOWN_MESSAGE)) {
        StrictDeleteOutcomeV1::AlreadyGone
    } else if (400..500).contains(&status) {
        StrictDeleteOutcomeV1::DefinitelyNotApplied
    } else {
        StrictDeleteOutcomeV1::Ambiguous
    }
}

fn delete_failure(kind: &ErrorType) -> StrictDeleteOutcomeV1 {
    match kind {
        ErrorType::BuildingRequest
        | ErrorType::CreatingHeader { .. }
        | ErrorType::Json
        | ErrorType::Unauthorized
        | ErrorType::Validation => StrictDeleteOutcomeV1::DefinitelyNotApplied,
        ErrorType::Response { error, status, .. } => {
            delete_response_failure(status.get(), response_code(error))
        }
        _ => StrictDeleteOutcomeV1::Ambiguous,
    }
}

impl StrictPanelInstaller for TwilightStrictPanelInstaller<'_> {
    async fn observe_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> Result<StrictObservedMessageV1, InstallerError> {
        if channel_id.0 == 0 || message_id.0 == 0 {
            return Err(InstallerError::new(
                "strict panel observation id is invalid",
            ));
        }
        let response = match self
            .http
            .message(Id::new(channel_id.0), Id::new(message_id.0))
            .await
        {
            Ok(response) => response,
            Err(error) if observe_missing(error.kind()) => {
                return Ok(StrictObservedMessageV1::Missing);
            }
            Err(_) => return Err(InstallerError::new("strict panel observation failed")),
        };
        let message = response
            .model()
            .await
            .map_err(|_| InstallerError::new("strict panel observation decode failed"))?;
        Ok(StrictObservedMessageV1::Present(observed_payload(
            &message.content,
            &message.components,
        )))
    }

    async fn post_message(
        &self,
        channel_id: ChannelId,
        guild_id: GuildId,
        ruleset_key: &str,
        panel: &StrictDeclaredPanelV1,
    ) -> StrictExternalPostResultV1 {
        if channel_id.0 == 0 {
            return StrictExternalPostResultV1::DefinitelyNotApplied;
        }
        let expected = match render_strict_declared_panel_v1(guild_id, ruleset_key, &panel.spec) {
            Ok(expected) if expected == *panel => expected,
            _ => return StrictExternalPostResultV1::DefinitelyNotApplied,
        };
        let components = match build_components(&expected.expected_payload) {
            Ok(components) => components,
            Err(_) => return StrictExternalPostResultV1::DefinitelyNotApplied,
        };
        let response = match self
            .http
            .create_message(Id::new(channel_id.0))
            .content(&expected.expected_payload.content)
            .components(&components)
            .await
        {
            Ok(response) => response,
            Err(error) => return post_failure(error.kind()),
        };
        match response.model().await {
            Ok(message) => StrictExternalPostResultV1::Applied(MessageId(message.id.get())),
            Err(_) => StrictExternalPostResultV1::Ambiguous,
        }
    }

    async fn delete_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> StrictDeleteOutcomeV1 {
        if channel_id.0 == 0 || message_id.0 == 0 {
            return StrictDeleteOutcomeV1::DefinitelyNotApplied;
        }
        match self
            .http
            .delete_message(Id::new(channel_id.0), Id::new(message_id.0))
            .await
        {
            Ok(_) => StrictDeleteOutcomeV1::Deleted,
            Err(error) => delete_failure(error.kind()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_state::{ButtonSpec, InstanceRef};
    use desired_state::ResourceKey;

    fn panel(route: ButtonRoute) -> PanelSpec {
        PanelSpec {
            key: "entry".to_string(),
            channel: ResourceKey("hub".to_string()),
            content: "Welcome".to_string(),
            buttons: vec![ButtonSpec {
                label: "Join".to_string(),
                route,
            }],
        }
    }

    #[test]
    fn renderer_matches_the_exact_twilight_payload() {
        let declared = render_strict_declared_panel_v1(
            GuildId(7),
            "studyroom",
            &panel(ButtonRoute::Static {
                key: "join".to_string(),
            }),
        )
        .unwrap();
        let components = build_components(&declared.expected_payload).unwrap();
        assert_eq!(
            observed_payload(&declared.expected_payload.content, &components),
            declared.expected_payload
        );
    }

    #[test]
    fn renderer_rejects_routes_that_require_an_instance() {
        assert!(render_strict_declared_panel_v1(
            GuildId(7),
            "studyroom",
            &panel(ButtonRoute::InstanceAction {
                instance: InstanceRef::Event,
                action: "join".to_string(),
            }),
        )
        .is_err());
    }

    #[test]
    fn unsupported_observed_components_never_match_declared_payloads() {
        let declared = render_strict_declared_panel_v1(
            GuildId(7),
            "studyroom",
            &panel(ButtonRoute::Static {
                key: "join".to_string(),
            }),
        )
        .unwrap();
        let observed = observed_payload("Welcome", &[Component::Unknown(99)]);
        assert_ne!(observed, declared.expected_payload);
    }

    #[test]
    fn post_classification_is_conservative_after_dispatch() {
        assert_eq!(
            post_response_failure(400),
            StrictExternalPostResultV1::DefinitelyNotApplied
        );
        assert_eq!(
            post_response_failure(500),
            StrictExternalPostResultV1::Ambiguous
        );
        assert_eq!(
            post_failure(&ErrorType::RequestTimedOut),
            StrictExternalPostResultV1::Ambiguous
        );
    }

    #[test]
    fn delete_classification_is_idempotent_and_conservative() {
        assert_eq!(
            delete_response_failure(404, Some(UNKNOWN_MESSAGE)),
            StrictDeleteOutcomeV1::AlreadyGone
        );
        assert_eq!(
            delete_response_failure(403, Some(50_013)),
            StrictDeleteOutcomeV1::DefinitelyNotApplied
        );
        assert_eq!(
            delete_failure(&ErrorType::RequestError),
            StrictDeleteOutcomeV1::Ambiguous
        );
    }
}
