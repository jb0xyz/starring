use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Serialize;

use crate::errors::StructuredError;

use super::model::{
    ClosePolicyV1, FeatureConfigurationV1, FeatureId, IntentLocaleV1, IntentResolutionContext,
    IntentValue, IntentWorkspaceV1, ManagedPrivateRoomControlsDraftV1,
    ManagedPrivateRoomCopyDraftV1, ManagedPrivateRoomDraftV1, ManagedPrivateRoomNamingDraftV1,
    MissingDecision, MissingDecisionKind, ResolvedCloseControlV1, ResolvedFeatureConfigurationV1,
    ResolvedFeatureIntentV1, ResolvedHelpControlV1, ResolvedIntentV1, ResolvedJoinControlV1,
    ResolvedManagedPrivateRoomControlsV1, ResolvedManagedPrivateRoomCopyV1,
    ResolvedManagedPrivateRoomNamingV1, ResolvedManagedPrivateRoomV1, RoomNamePatternV1,
    INTENT_SCHEMA_VERSION, PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ValidatedIntentV1(ResolvedIntentV1);

impl ValidatedIntentV1 {
    pub fn schema_version(&self) -> u16 {
        self.0.schema_version
    }

    pub fn revision(&self) -> u64 {
        self.0.revision
    }

    pub fn objective(&self) -> &str {
        &self.0.objective
    }

    pub fn requested_outcome(&self) -> super::model::IntentRequestedOutcome {
        self.0.requested_outcome
    }

    pub(super) fn resolved(&self) -> &ResolvedIntentV1 {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum IntentResolutionV1 {
    NeedsInput {
        workspace: IntentWorkspaceV1,
        decisions: Vec<MissingDecision>,
    },
    Resolved {
        intent: ValidatedIntentV1,
    },
}

const MAX_FEATURES: usize = 1;
const MAX_OBJECTIVE_CHARS: usize = 2_048;
const MAX_FEATURE_ID_CHARS: usize = 32;
const MAX_BINDING_KEY_CHARS: usize = 64;
const MAX_MESSAGE_CHARS: usize = 2_000;
const MAX_BUTTON_LABEL_CHARS: usize = 80;
const MAX_MODAL_TEXT_CHARS: usize = 45;
const MAX_NAME_AFFIX_CHARS: usize = 80;

pub(super) fn resolve_intent_workspace(
    mut workspace: IntentWorkspaceV1,
    context: &IntentResolutionContext,
) -> Result<IntentResolutionV1, StructuredError> {
    if workspace.schema_version != INTENT_SCHEMA_VERSION {
        return Err(intent_error(
            "UNSUPPORTED_INTENT_SCHEMA_VERSION",
            "intent.schema_version",
            format!(
                "Intent schema version {} is not supported",
                workspace.schema_version
            ),
            format!("Use intent schema version {INTENT_SCHEMA_VERSION}"),
        ));
    }
    workspace.objective = normalized_required_text(
        &workspace.objective,
        MAX_OBJECTIVE_CHARS,
        true,
        false,
        "intent.objective",
    )?;
    if workspace.features.is_empty() {
        return Err(intent_error(
            "EMPTY_INTENT_FEATURES",
            "intent.features",
            "The intent does not contain a feature",
            "Add at least one supported feature",
        ));
    }
    if workspace.features.len() > MAX_FEATURES {
        return Err(intent_error(
            "TOO_MANY_INTENT_FEATURES",
            "intent.features",
            format!(
                "The intent contains {} features but the maximum is {MAX_FEATURES}",
                workspace.features.len()
            ),
            "Split the request into smaller design changes",
        ));
    }

    let mut feature_ids = BTreeSet::new();
    let mut decisions = Vec::new();
    let mut resolved_features = Vec::with_capacity(workspace.features.len());
    for (index, feature) in workspace.features.iter_mut().enumerate() {
        let feature_path = format!("intent.features.{index}");
        normalize_feature_id(&mut feature.feature_id, &feature_path)?;
        if !feature_ids.insert(feature.feature_id.clone()) {
            return Err(intent_error(
                "DUPLICATE_INTENT_FEATURE_ID",
                format!("{feature_path}.feature_id"),
                format!("Feature id {} is repeated", feature.feature_id.as_str()),
                "Use a unique stable feature id",
            ));
        }
        validate_recipe(&feature.recipe, &feature_path)?;
        let resolved = match &mut feature.configuration {
            FeatureConfigurationV1::ManagedPrivateRoom(configuration) => resolve_private_room(
                &feature.feature_id,
                configuration,
                &feature_path,
                context,
                &mut decisions,
            )?
            .map(ResolvedFeatureConfigurationV1::ManagedPrivateRoom),
        };
        if let Some(configuration) = resolved {
            resolved_features.push(ResolvedFeatureIntentV1 {
                feature_id: feature.feature_id.clone(),
                recipe: feature.recipe.clone(),
                configuration,
            });
        }
    }

    if decisions.is_empty() {
        Ok(IntentResolutionV1::Resolved {
            intent: ValidatedIntentV1(ResolvedIntentV1 {
                schema_version: workspace.schema_version,
                revision: workspace.revision,
                objective: workspace.objective,
                requested_outcome: workspace.requested_outcome,
                features: resolved_features,
            }),
        })
    } else {
        Ok(IntentResolutionV1::NeedsInput {
            workspace,
            decisions,
        })
    }
}

fn normalize_feature_id(
    feature_id: &mut FeatureId,
    feature_path: &str,
) -> Result<(), StructuredError> {
    feature_id.0 = feature_id.0.trim().to_string();
    let valid = !feature_id.0.is_empty()
        && feature_id.0.chars().count() <= MAX_FEATURE_ID_CHARS
        && feature_id
            .0
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_lowercase())
        && feature_id
            .0
            .chars()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == '_');
    if !valid {
        return Err(intent_error(
            "INVALID_INTENT_FEATURE_ID",
            format!("{feature_path}.feature_id"),
            format!("Feature id {:?} is invalid", feature_id.0),
            "Use 1 to 32 lowercase ASCII letters, digits, or underscores beginning with a letter",
        ));
    }
    Ok(())
}

fn validate_recipe(
    recipe: &super::model::RecipeRef,
    feature_path: &str,
) -> Result<(), StructuredError> {
    if recipe.id != PRIVATE_STUDY_ROOM_RECIPE_ID {
        return Err(intent_error(
            "UNKNOWN_INTENT_RECIPE",
            format!("{feature_path}.recipe.id"),
            format!("Recipe {} is not registered", recipe.id),
            format!("Use the registered recipe {PRIVATE_STUDY_ROOM_RECIPE_ID}"),
        ));
    }
    if recipe.version != PRIVATE_STUDY_ROOM_RECIPE_VERSION {
        return Err(intent_error(
            "UNKNOWN_INTENT_RECIPE_VERSION",
            format!("{feature_path}.recipe.version"),
            format!(
                "Recipe {} version {} is not registered",
                recipe.id, recipe.version
            ),
            format!("Use exact recipe version {PRIVATE_STUDY_ROOM_RECIPE_VERSION}"),
        ));
    }
    Ok(())
}

fn resolve_private_room(
    feature_id: &FeatureId,
    configuration: &mut ManagedPrivateRoomDraftV1,
    feature_path: &str,
    context: &IntentResolutionContext,
    decisions: &mut Vec<MissingDecision>,
) -> Result<Option<ResolvedManagedPrivateRoomV1>, StructuredError> {
    normalize_private_room(configuration, feature_path)?;
    let Some(hub_channel) = configuration.hub_channel.clone() else {
        decisions.push(MissingDecision {
            id: format!("{}.hub_channel", feature_id.as_str()),
            feature_id: feature_id.clone(),
            path: format!("{feature_path}.configuration.parameters.hub_channel"),
            kind: MissingDecisionKind::ExistingChannel,
            question: "Which existing channel should host the study-room launcher?".to_string(),
            reason: "The recipe must bind its launcher and discovery panel to one existing channel"
                .to_string(),
            options: context
                .channel_bindings
                .iter()
                .map(|key| key.as_str().to_string())
                .collect(),
        });
        return Ok(None);
    };
    if !context.channel_bindings.contains(&hub_channel.value) {
        return Err(intent_error(
            "UNKNOWN_INTENT_CHANNEL_BINDING",
            format!("{feature_path}.configuration.parameters.hub_channel.value"),
            format!(
                "Channel binding {} is not available",
                hub_channel.value.as_str()
            ),
            "Choose a channel binding from the deterministic context catalog",
        ));
    }

    let copy = configuration.copy.clone();
    let naming = configuration.naming.clone();
    let controls = configuration.controls.clone();
    let locale = configuration
        .locale
        .clone()
        .unwrap_or_else(|| IntentValue::recipe_default(IntentLocaleV1::En));
    let defaults = private_room_defaults(locale.value);
    Ok(Some(ResolvedManagedPrivateRoomV1 {
        hub_channel,
        locale: locale.clone(),
        copy: resolve_copy(copy, &defaults),
        naming: resolve_naming(naming, &defaults),
        controls: resolve_controls(controls, &defaults, feature_path)?,
    }))
}

fn normalize_private_room(
    configuration: &mut ManagedPrivateRoomDraftV1,
    feature_path: &str,
) -> Result<(), StructuredError> {
    let base = format!("{feature_path}.configuration.parameters");
    if let Some(hub_channel) = &mut configuration.hub_channel {
        hub_channel.value.0 = hub_channel.value.0.trim().to_string();
        let valid = !hub_channel.value.0.is_empty()
            && hub_channel.value.0.chars().count() <= MAX_BINDING_KEY_CHARS
            && hub_channel.value.0.chars().all(|value| {
                value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '.' | ':' | '/')
            });
        if !valid {
            return Err(intent_error(
                "INVALID_INTENT_CHANNEL_BINDING",
                format!("{base}.hub_channel.value"),
                format!("Channel binding key {:?} is invalid", hub_channel.value.0),
                "Use a non-empty existing binding key containing only ASCII letters, digits, _, -, ., :, or /",
            ));
        }
    }

    normalize_optional_text(
        &mut configuration.copy.launcher_content,
        MAX_MESSAGE_CHARS,
        true,
        &format!("{base}.copy.launcher_content"),
    )?;
    normalize_optional_text(
        &mut configuration.copy.create_button_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        &format!("{base}.copy.create_button_label"),
    )?;
    normalize_optional_text(
        &mut configuration.copy.modal_title,
        MAX_MODAL_TEXT_CHARS,
        false,
        &format!("{base}.copy.modal_title"),
    )?;
    normalize_optional_text(
        &mut configuration.copy.room_name_label,
        MAX_MODAL_TEXT_CHARS,
        false,
        &format!("{base}.copy.room_name_label"),
    )?;
    normalize_optional_pattern(
        &mut configuration.copy.welcome_content,
        MAX_MESSAGE_CHARS,
        true,
        &format!("{base}.copy.welcome_content"),
    )?;
    normalize_optional_pattern(
        &mut configuration.copy.hub_announcement,
        MAX_MESSAGE_CHARS,
        true,
        &format!("{base}.copy.hub_announcement"),
    )?;
    normalize_optional_pattern(
        &mut configuration.copy.completed_response,
        MAX_MESSAGE_CHARS,
        true,
        &format!("{base}.copy.completed_response"),
    )?;
    normalize_optional_pattern(
        &mut configuration.naming.channel_name,
        MAX_NAME_AFFIX_CHARS,
        false,
        &format!("{base}.naming.channel_name"),
    )?;
    normalize_optional_pattern(
        &mut configuration.naming.member_role_name,
        MAX_NAME_AFFIX_CHARS,
        false,
        &format!("{base}.naming.member_role_name"),
    )?;
    normalize_optional_text(
        &mut configuration.controls.help_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        &format!("{base}.controls.help_label"),
    )?;
    normalize_optional_text(
        &mut configuration.controls.help_response,
        MAX_MESSAGE_CHARS,
        true,
        &format!("{base}.controls.help_response"),
    )?;
    normalize_optional_text(
        &mut configuration.controls.join_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        &format!("{base}.controls.join_label"),
    )?;
    normalize_optional_text(
        &mut configuration.controls.joined_response,
        MAX_MESSAGE_CHARS,
        true,
        &format!("{base}.controls.joined_response"),
    )?;
    normalize_optional_text(
        &mut configuration.controls.close_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        &format!("{base}.controls.close_label"),
    )?;
    normalize_optional_text(
        &mut configuration.controls.closed_response,
        MAX_MESSAGE_CHARS,
        true,
        &format!("{base}.controls.closed_response"),
    )?;
    Ok(())
}

fn resolve_copy(
    copy: ManagedPrivateRoomCopyDraftV1,
    defaults: &PrivateRoomDefaults,
) -> ResolvedManagedPrivateRoomCopyV1 {
    ResolvedManagedPrivateRoomCopyV1 {
        launcher_content: copy
            .launcher_content
            .unwrap_or_else(|| IntentValue::recipe_default(defaults.launcher_content.to_string())),
        create_button_label: copy.create_button_label.unwrap_or_else(|| {
            IntentValue::recipe_default(defaults.create_button_label.to_string())
        }),
        modal_title: copy
            .modal_title
            .unwrap_or_else(|| IntentValue::recipe_default(defaults.modal_title.to_string())),
        room_name_label: copy
            .room_name_label
            .unwrap_or_else(|| IntentValue::recipe_default(defaults.room_name_label.to_string())),
        welcome_content: copy.welcome_content.unwrap_or_else(|| {
            IntentValue::recipe_default(pattern(defaults.welcome_prefix, defaults.welcome_suffix))
        }),
        hub_announcement: copy.hub_announcement.unwrap_or_else(|| {
            IntentValue::recipe_default(pattern(
                defaults.announcement_prefix,
                defaults.announcement_suffix,
            ))
        }),
        completed_response: copy.completed_response.unwrap_or_else(|| {
            IntentValue::recipe_default(pattern(
                defaults.completed_prefix,
                defaults.completed_suffix,
            ))
        }),
    }
}

fn resolve_naming(
    naming: ManagedPrivateRoomNamingDraftV1,
    defaults: &PrivateRoomDefaults,
) -> ResolvedManagedPrivateRoomNamingV1 {
    ResolvedManagedPrivateRoomNamingV1 {
        channel_name: naming.channel_name.unwrap_or_else(|| {
            IntentValue::recipe_default(pattern(defaults.channel_prefix, defaults.channel_suffix))
        }),
        member_role_name: naming.member_role_name.unwrap_or_else(|| {
            IntentValue::recipe_default(pattern(defaults.role_prefix, defaults.role_suffix))
        }),
    }
}

fn resolve_controls(
    controls: ManagedPrivateRoomControlsDraftV1,
    defaults: &PrivateRoomDefaults,
    feature_path: &str,
) -> Result<ResolvedManagedPrivateRoomControlsV1, StructuredError> {
    let help = ResolvedHelpControlV1 {
        label: controls
            .help_label
            .unwrap_or_else(|| IntentValue::recipe_default(defaults.help_label.to_string())),
        response: controls
            .help_response
            .unwrap_or_else(|| IntentValue::recipe_default(defaults.help_response.to_string())),
    };
    let join = ResolvedJoinControlV1 {
        label: controls
            .join_label
            .unwrap_or_else(|| IntentValue::recipe_default(defaults.join_label.to_string())),
        response: controls
            .joined_response
            .unwrap_or_else(|| IntentValue::recipe_default(defaults.joined_response.to_string())),
    };
    let close_policy = controls
        .close_policy
        .unwrap_or_else(|| IntentValue::recipe_default(ClosePolicyV1::Disabled));
    let close = match close_policy.value {
        ClosePolicyV1::Disabled => {
            if controls.close_label.is_some() || controls.closed_response.is_some() {
                return Err(intent_error(
                    "INACTIVE_INTENT_CLOSE_SLOT",
                    format!("{feature_path}.configuration.parameters.controls"),
                    "Close copy was provided while the close control is disabled",
                    "Remove close copy or explicitly choose the any_member close policy",
                ));
            }
            ResolvedCloseControlV1::Disabled {
                source: close_policy.source,
            }
        }
        ClosePolicyV1::AnyMember => ResolvedCloseControlV1::AnyMember {
            source: close_policy.source,
            label: controls
                .close_label
                .unwrap_or_else(|| IntentValue::recipe_default(defaults.close_label.to_string())),
            response: controls.closed_response.unwrap_or_else(|| {
                IntentValue::recipe_default(defaults.closed_response.to_string())
            }),
        },
    };
    Ok(ResolvedManagedPrivateRoomControlsV1 { help, join, close })
}

#[derive(Clone, Copy)]
struct PrivateRoomDefaults {
    launcher_content: &'static str,
    create_button_label: &'static str,
    modal_title: &'static str,
    room_name_label: &'static str,
    welcome_prefix: &'static str,
    welcome_suffix: &'static str,
    announcement_prefix: &'static str,
    announcement_suffix: &'static str,
    completed_prefix: &'static str,
    completed_suffix: &'static str,
    channel_prefix: &'static str,
    channel_suffix: &'static str,
    role_prefix: &'static str,
    role_suffix: &'static str,
    help_label: &'static str,
    help_response: &'static str,
    join_label: &'static str,
    joined_response: &'static str,
    close_label: &'static str,
    closed_response: &'static str,
}

fn private_room_defaults(locale: IntentLocaleV1) -> PrivateRoomDefaults {
    match locale {
        IntentLocaleV1::En => PrivateRoomDefaults {
            launcher_content: "Create a study room",
            create_button_label: "Create room",
            modal_title: "Create study room",
            room_name_label: "Room name",
            welcome_prefix: "Welcome to ",
            welcome_suffix: "",
            announcement_prefix: "",
            announcement_suffix: " is open",
            completed_prefix: "Created ",
            completed_suffix: "",
            channel_prefix: "study-",
            channel_suffix: "",
            role_prefix: "",
            role_suffix: " members",
            help_label: "Help",
            help_response: "This is a private study room",
            join_label: "Join",
            joined_response: "Joined the study room",
            close_label: "Close",
            closed_response: "The study room was closed",
        },
        IntentLocaleV1::Ko => PrivateRoomDefaults {
            launcher_content: "스터디룸을 만들어보세요",
            create_button_label: "스터디룸 만들기",
            modal_title: "스터디룸 만들기",
            room_name_label: "방 이름",
            welcome_prefix: "",
            welcome_suffix: " 스터디룸에 오신 것을 환영합니다",
            announcement_prefix: "",
            announcement_suffix: " 스터디룸이 열렸습니다",
            completed_prefix: "",
            completed_suffix: " 스터디룸을 만들었습니다",
            channel_prefix: "study-",
            channel_suffix: "",
            role_prefix: "",
            role_suffix: " 멤버",
            help_label: "도움말",
            help_response: "멤버 역할이 있는 사용자만 볼 수 있는 비공개 스터디룸입니다",
            join_label: "참가하기",
            joined_response: "스터디룸에 참가했습니다",
            close_label: "닫기",
            closed_response: "스터디룸을 닫았습니다",
        },
    }
}

fn pattern(prefix: &str, suffix: &str) -> RoomNamePatternV1 {
    RoomNamePatternV1 {
        prefix: prefix.to_string(),
        suffix: suffix.to_string(),
    }
}

fn normalize_optional_text(
    slot: &mut Option<IntentValue<String>>,
    max_chars: usize,
    multiline: bool,
    path: &str,
) -> Result<(), StructuredError> {
    if let Some(slot) = slot {
        slot.value = normalized_required_text(
            &slot.value,
            max_chars,
            multiline,
            true,
            &format!("{path}.value"),
        )?;
    }
    Ok(())
}

fn normalize_optional_pattern(
    slot: &mut Option<IntentValue<RoomNamePatternV1>>,
    max_chars: usize,
    multiline: bool,
    path: &str,
) -> Result<(), StructuredError> {
    if let Some(slot) = slot {
        validate_affix(
            &slot.value.prefix,
            max_chars,
            multiline,
            &format!("{path}.value.prefix"),
        )?;
        validate_affix(
            &slot.value.suffix,
            max_chars,
            multiline,
            &format!("{path}.value.suffix"),
        )?;
        if slot.value.prefix.encode_utf16().count() + slot.value.suffix.encode_utf16().count()
            > max_chars
        {
            return Err(intent_error(
                "INTENT_TEXT_TOO_LONG",
                format!("{path}.value"),
                format!("The combined pattern exceeds {max_chars} characters"),
                "Shorten the pattern prefix or suffix",
            ));
        }
    }
    Ok(())
}

fn normalized_required_text(
    value: &str,
    max_chars: usize,
    multiline: bool,
    reject_template: bool,
    path: &str,
) -> Result<String, StructuredError> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(intent_error(
            "EMPTY_INTENT_TEXT",
            path,
            "An intent text value is empty",
            "Provide a non-empty semantic value",
        ));
    }
    validate_text_shape(&normalized, max_chars, multiline, reject_template, path)?;
    Ok(normalized)
}

fn validate_affix(
    value: &str,
    max_chars: usize,
    multiline: bool,
    path: &str,
) -> Result<(), StructuredError> {
    validate_text_shape(value, max_chars, multiline, true, path)
}

fn validate_text_shape(
    value: &str,
    max_chars: usize,
    multiline: bool,
    reject_template: bool,
    path: &str,
) -> Result<(), StructuredError> {
    if value.encode_utf16().count() > max_chars {
        return Err(intent_error(
            "INTENT_TEXT_TOO_LONG",
            path,
            format!("The intent text exceeds {max_chars} characters"),
            "Shorten the value",
        ));
    }
    if value.chars().any(|character| {
        (character.is_control() && !(multiline && character == '\n'))
            || is_directional_control(character)
    }) {
        return Err(intent_error(
            "INVALID_INTENT_TEXT_CONTROL",
            path,
            "The intent text contains an unsupported control character",
            "Remove line breaks or null characters from this value",
        ));
    }
    if reject_template && value.contains("${") {
        return Err(intent_error(
            "RAW_INTENT_TEMPLATE_FORBIDDEN",
            path,
            "Raw template syntax is not allowed in Intent IR",
            "Use the typed room-name prefix and suffix fields",
        ));
    }
    Ok(())
}

fn is_directional_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

fn intent_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
