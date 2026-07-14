use std::borrow::Cow;
use std::collections::BTreeSet;

use schemars::{schema_for, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{ExistingChannelKey, IntentRequestedOutcome};
use crate::tools::ToolDefinition;

use super::intent_interpretation::{
    CloseAuthorizationV2, IntentAutomationKindV2, IntentBoundaryRequestV2, IntentLocaleHintV2,
    IntentRequestModeV2, RuntimeRequirementsV2,
};
use super::intent_text::{normalized_required_text, validate_text_shape};

pub const INTERPRET_INTENT_CORE: &str = "interpret_intent_core";

const MAX_OBJECTIVE_CHARS: usize = 2_048;
const MAX_RESPONSE_CHARS: usize = 2_000;
const MAX_UNCLASSIFIED_REQUIREMENTS: usize = 8;
const MAX_UNCLASSIFIED_REQUIREMENT_CHARS: usize = 160;
const MAX_BINDING_KEY_CHARS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct RequiredNullableChannelV3(Option<ExistingChannelKey>);

impl JsonSchema for RequiredNullableChannelV3 {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "RequiredNullableChannelV3".into()
    }

    fn schema_id() -> Cow<'static, str> {
        concat!(module_path!(), "::RequiredNullableChannelV3").into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        <Option<ExistingChannelKey>>::json_schema(generator)
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum IntentRecipeDetailFacetV3 {
    Copy,
    Naming,
    Controls,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InterpretIntentCoreWireV3 {
    expected_revision: u64,
    request_mode: IntentRequestModeV2,
    automation_kind: IntentAutomationKindV2,
    #[schemars(length(min = 1, max = 2048))]
    objective: String,
    requested_outcome: IntentRequestedOutcome,
    #[serde(deserialize_with = "deserialize_required_nullable_channel")]
    #[schemars(required)]
    hub_channel: RequiredNullableChannelV3,
    locale: IntentLocaleHintV2,
    close_authorization: CloseAuthorizationV2,
    runtime_requirements: RuntimeRequirementsV2,
    #[schemars(length(max = 3))]
    boundary_requests: Vec<IntentBoundaryRequestV2>,
    #[schemars(length(max = 8), inner(length(min = 1, max = 160)))]
    unclassified_requirements: Vec<String>,
    #[schemars(length(max = 3))]
    detail_facets: Vec<IntentRecipeDetailFacetV3>,
    #[schemars(length(max = 2000))]
    response: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentCoreInterpretationV3(InterpretIntentCoreWireV3);

impl IntentCoreInterpretationV3 {
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

    pub fn selected_existing_channel(&self) -> Option<&ExistingChannelKey> {
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

    pub fn recipe_detail_facets(&self) -> &[IntentRecipeDetailFacetV3] {
        &self.0.detail_facets
    }

    pub fn response(&self) -> &str {
        &self.0.response
    }
}

pub fn interpret_intent_core_frontier() -> [ToolDefinition; 1] {
    [ToolDefinition {
        name: INTERPRET_INTENT_CORE.to_string(),
        description: "Extract one bounded Core IR from the human turn. Preserve every runtime, authorization, lifecycle, external-effect, safety-boundary, and explicit recipe-detail requirement without choosing routes, capability identifiers, recipes, actions, or deployment operations".to_string(),
        parameters: schema_value::<InterpretIntentCoreWireV3>(),
    }]
}

pub fn parse_interpret_intent_core(
    arguments: &str,
) -> Result<IntentCoreInterpretationV3, StructuredError> {
    let input = serde_json::from_str::<InterpretIntentCoreWireV3>(arguments).map_err(|error| {
        translate_tool_arguments_error(
            INTERPRET_INTENT_CORE,
            &error,
            &schema_value::<InterpretIntentCoreWireV3>(),
        )
    })?;
    normalize_core(input).map(IntentCoreInterpretationV3)
}

fn deserialize_required_nullable_channel<'de, D>(
    deserializer: D,
) -> Result<RequiredNullableChannelV3, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<ExistingChannelKey>::deserialize(deserializer).map(RequiredNullableChannelV3)
}

fn normalize_core(
    mut input: InterpretIntentCoreWireV3,
) -> Result<InterpretIntentCoreWireV3, StructuredError> {
    validate_mode_outcome(&input)?;
    input.objective = normalized_required_text(
        &input.objective,
        MAX_OBJECTIVE_CHARS,
        true,
        false,
        "intent.core.objective",
    )?;
    input.response = normalized_response(&input.response, input.request_mode)?;
    normalize_channel(&mut input.hub_channel)?;
    input.unclassified_requirements = normalize_requirements(
        input.unclassified_requirements,
        MAX_UNCLASSIFIED_REQUIREMENTS,
        MAX_UNCLASSIFIED_REQUIREMENT_CHARS,
        "intent.core.unclassified_requirements",
    )?;
    if input.boundary_requests.len() > 3 {
        return Err(core_error(
            "TOO_MANY_INTENT_BOUNDARY_REQUESTS",
            "intent.core.boundary_requests",
            "The interpretation contains more than three safety boundary requests",
            "Use only the closed safety boundary identifiers",
        ));
    }
    if input.detail_facets.len() > 3 {
        return Err(core_error(
            "TOO_MANY_RECIPE_DETAIL_FACETS",
            "intent.core.detail_facets",
            "The interpretation contains more than three recipe detail facets",
            "Use only copy, naming, and controls",
        ));
    }
    input.detail_facets = input
        .detail_facets
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !input.detail_facets.is_empty()
        && (input.request_mode != IntentRequestModeV2::Build
            || input.automation_kind != IntentAutomationKindV2::ManagedPrivateStudyRoom)
    {
        return Err(core_error(
            "INCONSISTENT_INTENT_CORE",
            "intent.core.detail_facets",
            "Recipe detail facets require a managed private study-room build",
            "Use detail facets only for explicit copy, naming, or control customization of that recipe",
        ));
    }
    input.boundary_requests = input
        .boundary_requests
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(input)
}

fn validate_mode_outcome(input: &InterpretIntentCoreWireV3) -> Result<(), StructuredError> {
    if input.request_mode == IntentRequestModeV2::Discussion
        && input.automation_kind != IntentAutomationKindV2::None
    {
        return Err(core_error(
            "INCONSISTENT_INTENT_CORE",
            "intent.core.automation_kind",
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
        (IntentRequestModeV2::Discussion, _) => Err(core_error(
            "INCONSISTENT_INTENT_CORE",
            "intent.core.requested_outcome",
            "A discussion request must use the discussion outcome",
            "Set requested_outcome to discussion",
        )),
        (IntentRequestModeV2::Build, IntentRequestedOutcome::Discussion) => Err(core_error(
            "INCONSISTENT_INTENT_CORE",
            "intent.core.requested_outcome",
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
        return Err(core_error(
            "EMPTY_INTENT_TEXT",
            "intent.core.response",
            "A discussion response is empty",
            "Provide one natural response to the human",
        ));
    }
    validate_text_shape(
        &normalized,
        MAX_RESPONSE_CHARS,
        true,
        false,
        "intent.core.response",
    )?;
    Ok(normalized)
}

fn normalize_channel(channel: &mut RequiredNullableChannelV3) -> Result<(), StructuredError> {
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
    Err(core_error(
        "INVALID_INTENT_CHANNEL_BINDING",
        "intent.core.hub_channel",
        "The selected channel binding key is invalid",
        "Use a non-empty existing binding key containing only ASCII letters, digits, _, -, ., :, or /",
    ))
}

fn normalize_requirements(
    values: Vec<String>,
    maximum: usize,
    max_chars: usize,
    path: &str,
) -> Result<Vec<String>, StructuredError> {
    if values.len() > maximum {
        return Err(core_error(
            "TOO_MANY_INTENT_REQUIREMENTS",
            path,
            format!("The interpretation contains more than {maximum} requirements"),
            "Keep only distinct requirements not represented by closed fields",
        ));
    }
    let mut normalized = BTreeSet::new();
    for (index, value) in values.into_iter().enumerate() {
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        normalized.insert(normalized_required_text(
            &value,
            max_chars,
            false,
            false,
            &format!("{path}.{index}"),
        )?);
    }
    Ok(normalized.into_iter().collect())
}

fn schema_value<T: JsonSchema>() -> Value {
    let mut value = serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({}));
    if let Some(root) = value.as_object_mut() {
        root.remove("$schema");
        root.remove("title");
        if let Some(expected_revision) = root
            .get_mut("properties")
            .and_then(Value::as_object_mut)
            .and_then(|properties| properties.get_mut("expected_revision"))
            .and_then(Value::as_object_mut)
        {
            expected_revision.remove("format");
            expected_revision.remove("minimum");
        }
    }
    value
}

fn core_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError {
        code: code.into(),
        location: location.into(),
        message: message.into(),
        hint: hint.into(),
    }
}
