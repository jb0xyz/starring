use std::borrow::Cow;
use std::collections::BTreeSet;

use schemars::{schema_for, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{
    ExistingChannelKey, IntentRequestedOutcome, PrivateStudyRoomCopyProposalV1,
    PrivateStudyRoomNamingProposalV1, RoomNamePatternV1,
};
use crate::tools::ToolDefinition;

use super::intent_text::{normalized_required_text, validate_text_shape};

pub const INTERPRET_INTENT_TURN: &str = "interpret_intent_turn";

const MAX_OBJECTIVE_CHARS: usize = 2_048;
const MAX_RESPONSE_CHARS: usize = 2_000;
const MAX_UNCLASSIFIED_REQUIREMENTS: usize = 8;
const MAX_UNCLASSIFIED_REQUIREMENT_CHARS: usize = 160;
const MAX_BINDING_KEY_CHARS: usize = 64;
const MAX_BUTTON_LABEL_CHARS: usize = 80;
const MAX_MODAL_TEXT_CHARS: usize = 45;
const MAX_NAME_AFFIX_CHARS: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct RequiredNullableChannelV2(Option<ExistingChannelKey>);

impl JsonSchema for RequiredNullableChannelV2 {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "RequiredNullableChannelV2".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::RequiredNullableChannelV2").into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        <Option<ExistingChannelKey>>::json_schema(generator)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntentRequestModeV2 {
    Discussion,
    Build,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntentAutomationKindV2 {
    ManagedPrivateStudyRoom,
    CustomAutomation,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntentLocaleHintV2 {
    En,
    Ko,
    Unspecified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CloseAuthorizationV2 {
    NotRequested,
    Disabled,
    AnyMember,
    CreatorOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceRequirementV2 {
    None,
    RestartPersistent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimerRequirementV2 {
    None,
    Durable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EconomyRequirementV2 {
    None,
    PersistentLedger,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum IntentBoundaryRequestV2 {
    DirectLiveMutation,
    BypassValidationPreviewApproval,
    SecretDisclosure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRequirementsV2 {
    pub persistence: PersistenceRequirementV2,
    pub timers: TimerRequirementV2,
    pub economy: EconomyRequirementV2,
    pub event_time_llm: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivateStudyRoomControlsInterpretationV2 {
    #[serde(default)]
    pub help_label: Option<String>,
    #[serde(default)]
    pub help_response: Option<String>,
    #[serde(default)]
    pub join_label: Option<String>,
    #[serde(default)]
    pub joined_response: Option<String>,
    #[serde(default)]
    pub close_label: Option<String>,
    #[serde(default)]
    pub closed_response: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InterpretIntentTurnWireV2 {
    expected_revision: u64,
    request_mode: IntentRequestModeV2,
    automation_kind: IntentAutomationKindV2,
    #[schemars(
        length(min = 1, max = 2048),
        description = "Concise summary of the complete human request"
    )]
    objective: String,
    requested_outcome: IntentRequestedOutcome,
    #[serde(deserialize_with = "deserialize_required_nullable_channel")]
    #[schemars(
        required,
        description = "Exact existing channel key selected by the human, or null"
    )]
    hub_channel: RequiredNullableChannelV2,
    locale: IntentLocaleHintV2,
    close_authorization: CloseAuthorizationV2,
    runtime_requirements: RuntimeRequirementsV2,
    #[schemars(length(max = 3))]
    boundary_requests: Vec<IntentBoundaryRequestV2>,
    #[schemars(
        length(max = 8),
        inner(length(min = 1, max = 160)),
        description = "Hard runtime, authorization, lifecycle, or external-effect requirements not represented by the closed fields"
    )]
    unclassified_requirements: Vec<String>,
    #[schemars(
        length(max = 2000),
        description = "Natural answer for discussion; use an empty string for build turns"
    )]
    response: String,
    response_locale: IntentLocaleHintV2,
    #[serde(default)]
    copy: PrivateStudyRoomCopyProposalV1,
    #[serde(default)]
    naming: PrivateStudyRoomNamingProposalV1,
    #[serde(default)]
    controls: PrivateStudyRoomControlsInterpretationV2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentInterpretationV2(InterpretIntentTurnWireV2);

impl IntentInterpretationV2 {
    pub fn expected_revision(&self) -> u64 {
        self.0.expected_revision
    }

    pub fn request_mode(&self) -> IntentRequestModeV2 {
        self.0.request_mode
    }

    pub fn automation_kind(&self) -> IntentAutomationKindV2 {
        self.0.automation_kind
    }

    pub fn objective(&self) -> &str {
        &self.0.objective
    }

    pub fn requested_outcome(&self) -> IntentRequestedOutcome {
        self.0.requested_outcome
    }

    pub fn hub_channel(&self) -> Option<&ExistingChannelKey> {
        self.0.hub_channel.0.as_ref()
    }

    pub fn locale(&self) -> IntentLocaleHintV2 {
        self.0.locale
    }

    pub fn close_authorization(&self) -> CloseAuthorizationV2 {
        self.0.close_authorization
    }

    pub fn runtime_requirements(&self) -> &RuntimeRequirementsV2 {
        &self.0.runtime_requirements
    }

    pub fn boundary_requests(&self) -> &[IntentBoundaryRequestV2] {
        &self.0.boundary_requests
    }

    pub fn unclassified_requirements(&self) -> &[String] {
        &self.0.unclassified_requirements
    }

    pub fn response(&self) -> &str {
        &self.0.response
    }

    pub fn response_locale(&self) -> IntentLocaleHintV2 {
        self.0.response_locale
    }

    pub fn copy(&self) -> &PrivateStudyRoomCopyProposalV1 {
        &self.0.copy
    }

    pub fn naming(&self) -> &PrivateStudyRoomNamingProposalV1 {
        &self.0.naming
    }

    pub fn controls(&self) -> &PrivateStudyRoomControlsInterpretationV2 {
        &self.0.controls
    }
}

pub fn interpret_intent_turn_frontier() -> [ToolDefinition; 1] {
    [ToolDefinition {
        name: INTERPRET_INTENT_TURN.to_string(),
        description: "Extract one bounded semantic interpretation of the human turn. Do not choose a route or author capability identifiers. Classify every runtime, authorization, lifecycle, boundary, and unclassified hard requirement without weakening it. Use managed_private_study_room only when the whole build is that managed recipe; use custom_automation for other static designs and none for discussion or boundary-only requests. The harness deterministically chooses reject, discussion, capability gap, typed planner, or recipe after this call".to_string(),
        parameters: schema_value::<InterpretIntentTurnWireV2>(),
    }]
}

pub fn parse_interpret_intent_turn(
    arguments: &str,
) -> Result<IntentInterpretationV2, StructuredError> {
    let input = serde_json::from_str::<InterpretIntentTurnWireV2>(arguments).map_err(|error| {
        translate_tool_arguments_error(
            INTERPRET_INTENT_TURN,
            &error,
            &schema_value::<InterpretIntentTurnWireV2>(),
        )
    })?;
    normalize_interpretation(input).map(IntentInterpretationV2)
}

fn deserialize_required_nullable_channel<'de, D>(
    deserializer: D,
) -> Result<RequiredNullableChannelV2, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<ExistingChannelKey>::deserialize(deserializer).map(RequiredNullableChannelV2)
}

fn normalize_interpretation(
    mut input: InterpretIntentTurnWireV2,
) -> Result<InterpretIntentTurnWireV2, StructuredError> {
    validate_mode_outcome(&input)?;
    input.objective = normalized_required_text(
        &input.objective,
        MAX_OBJECTIVE_CHARS,
        true,
        false,
        "intent.interpretation.objective",
    )?;
    input.response = normalized_response(&input.response, input.request_mode)?;
    normalize_channel(&mut input.hub_channel)?;
    normalize_private_overrides(&mut input)?;
    if input.unclassified_requirements.len() > MAX_UNCLASSIFIED_REQUIREMENTS {
        return Err(intent_error(
            "TOO_MANY_UNCLASSIFIED_INTENT_REQUIREMENTS",
            "intent.interpretation.unclassified_requirements",
            format!(
                "The interpretation contains {} unclassified requirements; expected at most {MAX_UNCLASSIFIED_REQUIREMENTS}",
                input.unclassified_requirements.len()
            ),
            "Keep only distinct hard requirements not represented by the closed fields",
        ));
    }
    let mut unclassified = BTreeSet::new();
    for (index, value) in input.unclassified_requirements.into_iter().enumerate() {
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        let value = normalized_required_text(
            &value,
            MAX_UNCLASSIFIED_REQUIREMENT_CHARS,
            false,
            false,
            &format!("intent.interpretation.unclassified_requirements.{index}"),
        )?;
        unclassified.insert(value);
    }
    input.unclassified_requirements = unclassified.into_iter().collect();
    input.boundary_requests = input
        .boundary_requests
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(input)
}

fn validate_mode_outcome(input: &InterpretIntentTurnWireV2) -> Result<(), StructuredError> {
    if input.request_mode == IntentRequestModeV2::Discussion
        && input.automation_kind != IntentAutomationKindV2::None
    {
        return Err(intent_error(
            "INCONSISTENT_INTENT_INTERPRETATION",
            "intent.interpretation.automation_kind",
            "A discussion request cannot select a build automation kind",
            "Set automation_kind to none for discussion",
        ));
    }
    match (input.request_mode, input.requested_outcome) {
        (IntentRequestModeV2::Discussion, IntentRequestedOutcome::Discussion)
        | (
            IntentRequestModeV2::Build,
            IntentRequestedOutcome::WorkingDraft | IntentRequestedOutcome::ValidatedPreview,
        ) => Ok(()),
        (IntentRequestModeV2::Discussion, _) => Err(intent_error(
            "INCONSISTENT_INTENT_INTERPRETATION",
            "intent.interpretation.requested_outcome",
            "A discussion request must use the discussion outcome",
            "Set requested_outcome to discussion",
        )),
        (IntentRequestModeV2::Build, IntentRequestedOutcome::Discussion) => Err(intent_error(
            "INCONSISTENT_INTENT_INTERPRETATION",
            "intent.interpretation.requested_outcome",
            "A build request must use a build outcome",
            "Use working_draft or validated_preview",
        )),
    }
}

fn normalized_response(
    value: &str,
    request_mode: IntentRequestModeV2,
) -> Result<String, StructuredError> {
    let normalized = value.trim().to_string();
    if request_mode == IntentRequestModeV2::Discussion && normalized.is_empty() {
        return Err(intent_error(
            "EMPTY_INTENT_TEXT",
            "intent.interpretation.response",
            "A discussion response is empty",
            "Provide one natural response to the human",
        ));
    }
    validate_text_shape(
        &normalized,
        MAX_RESPONSE_CHARS,
        true,
        false,
        "intent.interpretation.response",
    )?;
    Ok(normalized)
}

fn normalize_channel(channel: &mut RequiredNullableChannelV2) -> Result<(), StructuredError> {
    let Some(channel) = channel.0.as_mut() else {
        return Ok(());
    };
    channel.0 = channel.0.trim().to_string();
    let valid = !channel.0.is_empty()
        && channel.0.chars().count() <= MAX_BINDING_KEY_CHARS
        && channel.0.chars().all(|value| {
            value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '.' | ':' | '/')
        });
    if valid {
        return Ok(());
    }
    Err(intent_error(
        "INVALID_INTENT_CHANNEL_BINDING",
        "intent.interpretation.hub_channel",
        "The selected channel binding key is invalid",
        "Use a non-empty existing binding key containing only ASCII letters, digits, _, -, ., :, or /",
    ))
}

fn normalize_private_overrides(
    input: &mut InterpretIntentTurnWireV2,
) -> Result<(), StructuredError> {
    normalize_optional_text(
        &mut input.copy.launcher_content,
        MAX_RESPONSE_CHARS,
        true,
        "intent.interpretation.copy.launcher_content",
    )?;
    normalize_optional_text(
        &mut input.copy.create_button_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        "intent.interpretation.copy.create_button_label",
    )?;
    normalize_optional_text(
        &mut input.copy.modal_title,
        MAX_MODAL_TEXT_CHARS,
        false,
        "intent.interpretation.copy.modal_title",
    )?;
    normalize_optional_text(
        &mut input.copy.room_name_label,
        MAX_MODAL_TEXT_CHARS,
        false,
        "intent.interpretation.copy.room_name_label",
    )?;
    normalize_optional_pattern(
        &mut input.copy.welcome_content,
        MAX_RESPONSE_CHARS,
        true,
        "intent.interpretation.copy.welcome_content",
    )?;
    normalize_optional_pattern(
        &mut input.copy.hub_announcement,
        MAX_RESPONSE_CHARS,
        true,
        "intent.interpretation.copy.hub_announcement",
    )?;
    normalize_optional_pattern(
        &mut input.copy.completed_response,
        MAX_RESPONSE_CHARS,
        true,
        "intent.interpretation.copy.completed_response",
    )?;
    normalize_optional_pattern(
        &mut input.naming.channel_name,
        MAX_NAME_AFFIX_CHARS,
        false,
        "intent.interpretation.naming.channel_name",
    )?;
    normalize_optional_pattern(
        &mut input.naming.member_role_name,
        MAX_NAME_AFFIX_CHARS,
        false,
        "intent.interpretation.naming.member_role_name",
    )?;
    normalize_optional_text(
        &mut input.controls.help_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        "intent.interpretation.controls.help_label",
    )?;
    normalize_optional_text(
        &mut input.controls.help_response,
        MAX_RESPONSE_CHARS,
        true,
        "intent.interpretation.controls.help_response",
    )?;
    normalize_optional_text(
        &mut input.controls.join_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        "intent.interpretation.controls.join_label",
    )?;
    normalize_optional_text(
        &mut input.controls.joined_response,
        MAX_RESPONSE_CHARS,
        true,
        "intent.interpretation.controls.joined_response",
    )?;
    normalize_optional_text(
        &mut input.controls.close_label,
        MAX_BUTTON_LABEL_CHARS,
        false,
        "intent.interpretation.controls.close_label",
    )?;
    normalize_optional_text(
        &mut input.controls.closed_response,
        MAX_RESPONSE_CHARS,
        true,
        "intent.interpretation.controls.closed_response",
    )
}

fn normalize_optional_text(
    value: &mut Option<String>,
    max_chars: usize,
    multiline: bool,
    path: &str,
) -> Result<(), StructuredError> {
    if let Some(value) = value {
        *value = normalized_required_text(value, max_chars, multiline, true, path)?;
    }
    Ok(())
}

fn normalize_optional_pattern(
    value: &mut Option<RoomNamePatternV1>,
    max_chars: usize,
    multiline: bool,
    path: &str,
) -> Result<(), StructuredError> {
    let Some(value) = value else {
        return Ok(());
    };
    validate_text_shape(
        &value.prefix,
        max_chars,
        multiline,
        true,
        &format!("{path}.prefix"),
    )?;
    validate_text_shape(
        &value.suffix,
        max_chars,
        multiline,
        true,
        &format!("{path}.suffix"),
    )?;
    if value.prefix.encode_utf16().count() + value.suffix.encode_utf16().count() > max_chars {
        return Err(intent_error(
            "INTENT_TEXT_TOO_LONG",
            path,
            format!("The combined pattern exceeds {max_chars} characters"),
            "Shorten the pattern prefix or suffix",
        ));
    }
    Ok(())
}

fn schema_value<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({}))
}

fn intent_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
