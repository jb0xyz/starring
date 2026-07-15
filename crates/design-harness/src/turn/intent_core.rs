use std::borrow::Cow;
use std::collections::BTreeSet;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{ExistingChannelKey, IntentRequestedOutcome};
use crate::tools::ToolDefinition;

use super::intent_boundary_grounding::analyze_safety_boundaries;
use super::intent_capability_grounding::{
    ground_unmapped_capability_evidence, CapabilityEvidenceGroundingError,
};
use super::intent_detail_requirement::analyze_private_study_room_details;
use super::intent_interpretation::{
    CloseAuthorizationV2, EconomyRequirementV2, IntentAutomationKindV2, IntentBoundaryRequestV2,
    IntentLocaleHintV2, IntentRequestModeV2, PersistenceRequirementV2, RuntimeRequirementsV2,
    TimerRequirementV2,
};
use super::intent_request_mode_grounding::grounded_request_controls;
use super::intent_text::{normalized_required_text, validate_text_shape};
use super::schema::inline_schema_value;

pub const INTERPRET_INTENT_CORE: &str = "interpret_intent_core";

const MAX_DISCUSSION_RESPONSE_CHARS: usize = 480;
const MAX_UNCLASSIFIED_REQUIREMENTS: usize = 8;
const MAX_UNCLASSIFIED_REQUIREMENT_CHARS: usize = 160;
const MAX_BINDING_KEY_CHARS: usize = 64;
const MAX_RUNTIME_REQUIREMENTS: usize = 4;

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

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
enum CustomDetailFacetWireV3 {
    #[serde(rename = "custom_copy")]
    Copy,
    #[serde(rename = "custom_naming")]
    Naming,
    #[serde(rename = "custom_controls")]
    Controls,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GateDispositionWireV3 {
    #[default]
    Enforce,
    Skip,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum LiveDiscordMutationWireV3 {
    #[serde(rename = "no_live_mutation")]
    #[default]
    None,
    #[serde(rename = "mutate_live_now")]
    MutateLiveNow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum SecretDisclosureWireV3 {
    #[serde(rename = "no_secret_disclosure")]
    #[default]
    None,
    #[serde(rename = "disclose_secret_value")]
    DiscloseSecretValue,
}

impl From<CustomDetailFacetWireV3> for IntentRecipeDetailFacetV3 {
    fn from(value: CustomDetailFacetWireV3) -> Self {
        match value {
            CustomDetailFacetWireV3::Copy => Self::Copy,
            CustomDetailFacetWireV3::Naming => Self::Naming,
            CustomDetailFacetWireV3::Controls => Self::Controls,
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
enum RuntimeRequirementV3 {
    RestartPersistent,
    DurableTimer,
    PersistentEconomy,
    EventTimeLlm,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InterpretIntentCoreWireV4 {
    expected_revision: u64,
    request_mode: IntentRequestModeV2,
    automation_kind: IntentAutomationKindV2,
    requested_outcome: IntentRequestedOutcome,
    #[serde(deserialize_with = "deserialize_required_nullable_channel")]
    #[schemars(required)]
    hub_channel: RequiredNullableChannelV3,
    language: IntentLocaleHintV2,
    close_policy: CloseAuthorizationV2,
    #[schemars(length(max = 4))]
    runtime_requirements: Vec<RuntimeRequirementV3>,
    #[serde(default)]
    #[schemars(skip)]
    validation_gate: GateDispositionWireV3,
    #[serde(default)]
    #[schemars(skip)]
    preview_gate: GateDispositionWireV3,
    #[serde(default)]
    #[schemars(skip)]
    approval_gate: GateDispositionWireV3,
    #[serde(default)]
    #[schemars(skip)]
    live_discord_mutation: LiveDiscordMutationWireV3,
    #[serde(default)]
    #[schemars(skip)]
    secret_disclosure: SecretDisclosureWireV3,
    #[schemars(length(max = 8), inner(length(min = 1, max = 160)))]
    other_unmapped_required_capabilities: Vec<String>,
    #[serde(default)]
    #[schemars(skip)]
    custom_detail_facets: Vec<CustomDetailFacetWireV3>,
    #[schemars(length(max = 480))]
    response: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentCoreInterpretationV4 {
    expected_revision: u64,
    request_mode: IntentRequestModeV2,
    automation_kind: IntentAutomationKindV2,
    requested_outcome: IntentRequestedOutcome,
    hub_channel: RequiredNullableChannelV3,
    locale: IntentLocaleHintV2,
    close_authorization: CloseAuthorizationV2,
    runtime_requirements: RuntimeRequirementsV2,
    boundary_requests: Vec<IntentBoundaryRequestV2>,
    unclassified_requirements: Vec<String>,
    detail_facets: Vec<IntentRecipeDetailFacetV3>,
    response: String,
}

impl IntentCoreInterpretationV4 {
    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub fn request_mode(&self) -> IntentRequestModeV2 {
        self.request_mode
    }

    pub fn automation_kind(&self) -> IntentAutomationKindV2 {
        self.automation_kind
    }

    pub fn requested_outcome(&self) -> IntentRequestedOutcome {
        self.requested_outcome
    }

    pub fn selected_existing_channel(&self) -> Option<&ExistingChannelKey> {
        self.hub_channel.0.as_ref()
    }

    pub(crate) fn apply_human_grounded_channel(
        &mut self,
        grounded_channel: Option<&ExistingChannelKey>,
    ) {
        self.hub_channel.0 = grounded_channel.cloned();
    }

    pub(crate) fn apply_human_grounding(
        &mut self,
        human_message: &str,
        grounded_channel: Option<&ExistingChannelKey>,
    ) -> Result<(), StructuredError> {
        let canonical_human = canonical_human_message(human_message);
        let mut unclassified_requirements =
            grounded_capability_evidence(&canonical_human, self.unclassified_requirements.clone())?;
        let managed_private_study_room = self.request_mode == IntentRequestModeV2::Build
            && self.automation_kind == IntentAutomationKindV2::ManagedPrivateStudyRoom;
        let detail_facets = if managed_private_study_room {
            let analysis = analyze_private_study_room_details(human_message);
            unclassified_requirements
                .retain(|requirement| !analysis.explains_requirement(requirement));
            analysis.facets().to_vec()
        } else {
            Vec::new()
        };
        let boundary_analysis = analyze_safety_boundaries(human_message);
        unclassified_requirements
            .retain(|requirement| !boundary_analysis.owns_capability_evidence(requirement));
        let boundary_requests = boundary_analysis.requests().to_vec();
        let grounded_channel = if self.request_mode == IntentRequestModeV2::Build {
            grounded_channel
        } else {
            None
        };
        self.apply_human_grounded_channel(grounded_channel);
        self.boundary_requests = boundary_requests;
        self.unclassified_requirements = unclassified_requirements;
        self.detail_facets = detail_facets;
        Ok(())
    }

    pub fn locale(&self) -> IntentLocaleHintV2 {
        self.locale
    }

    pub fn close_authorization(&self) -> CloseAuthorizationV2 {
        self.close_authorization
    }

    pub fn runtime_requirements(&self) -> &RuntimeRequirementsV2 {
        &self.runtime_requirements
    }

    pub fn boundary_requests(&self) -> &[IntentBoundaryRequestV2] {
        &self.boundary_requests
    }

    pub fn unclassified_requirements(&self) -> &[String] {
        &self.unclassified_requirements
    }

    pub fn validate_human_evidence(&self, human_message: &str) -> Result<(), StructuredError> {
        let mut candidate = self.clone();
        candidate.apply_human_grounding(human_message, None)
    }

    pub fn recipe_detail_facets(&self) -> &[IntentRecipeDetailFacetV3] {
        &self.detail_facets
    }

    pub fn response(&self) -> &str {
        &self.response
    }
}

fn canonical_human_message(human_message: &str) -> String {
    human_message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn grounded_capability_evidence(
    human_message: &str,
    candidates: Vec<String>,
) -> Result<Vec<String>, StructuredError> {
    ground_unmapped_capability_evidence(
        human_message,
        candidates,
        MAX_UNCLASSIFIED_REQUIREMENT_CHARS,
    )
    .map_err(|error| match error {
        CapabilityEvidenceGroundingError::Ambiguous => core_error(
            "UNGROUNDED_INTENT_CAPABILITY_EVIDENCE",
            "intent.core.other_unmapped_required_capabilities",
            "An unmapped capability has multiple exact occurrences and cannot be repaired uniquely",
            "Return one complete exact source phrase including its leading article",
        ),
        CapabilityEvidenceGroundingError::ExpandedTooLong => core_error(
            "UNGROUNDED_INTENT_CAPABILITY_EVIDENCE",
            "intent.core.other_unmapped_required_capabilities",
            "An exact grounded capability exceeds the supported UTF-16 length after repair",
            "Return a shorter complete exact source phrase within 160 UTF-16 code units",
        ),
        CapabilityEvidenceGroundingError::Ungrounded => core_error(
            "UNGROUNDED_INTENT_CAPABILITY_EVIDENCE",
            "intent.core.other_unmapped_required_capabilities",
            "An unmapped capability is not an exact phrase from the human request",
            "Copy a contiguous human phrase without synthesizing an identifier",
        ),
    })
}

pub fn interpret_intent_core_frontier() -> [ToolDefinition; 1] {
    [ToolDefinition {
        name: INTERPRET_INTENT_CORE.to_string(),
        description: "Call once for every request, including discussion; put a concise complete conversational answer only in response and copy capability evidence exactly"
            .to_string(),
        parameters: inline_schema_value::<InterpretIntentCoreWireV4>(),
    }]
}

pub fn parse_interpret_intent_core(
    arguments: &str,
) -> Result<IntentCoreInterpretationV4, StructuredError> {
    normalize_core(parse_core_wire(arguments)?)
}

pub(crate) fn parse_interpret_intent_core_for_human(
    arguments: &str,
    human_message: &str,
) -> Result<IntentCoreInterpretationV4, StructuredError> {
    let grounded = grounded_request_controls(human_message);
    let mut input = match parse_core_wire(arguments) {
        Ok(input) => input,
        Err(error)
            if grounded.mode == Some(IntentRequestModeV2::Discussion)
                && missing_discussion_array(&error) =>
        {
            parse_core_wire(&supply_empty_discussion_arrays(arguments)?)?
        }
        Err(error) => return Err(error),
    };
    apply_grounded_request_mode(&mut input, grounded.mode, grounded.preview);
    normalize_core(input)
}

fn parse_core_wire(arguments: &str) -> Result<InterpretIntentCoreWireV4, StructuredError> {
    serde_json::from_str::<InterpretIntentCoreWireV4>(arguments).map_err(|error| {
        translate_tool_arguments_error(
            INTERPRET_INTENT_CORE,
            &error,
            &inline_schema_value::<InterpretIntentCoreWireV4>(),
        )
    })
}

fn missing_discussion_array(error: &StructuredError) -> bool {
    error.code == "MISSING_REQUIRED_FIELD"
        && [
            "tool.interpret_intent_core.arguments.runtime_requirements",
            "tool.interpret_intent_core.arguments.other_unmapped_required_capabilities",
        ]
        .contains(&error.location.as_str())
}

fn supply_empty_discussion_arrays(arguments: &str) -> Result<String, StructuredError> {
    let mut value = serde_json::from_str::<serde_json::Value>(arguments).map_err(|error| {
        translate_tool_arguments_error(
            INTERPRET_INTENT_CORE,
            &error,
            &inline_schema_value::<InterpretIntentCoreWireV4>(),
        )
    })?;
    let object = value.as_object_mut().ok_or_else(|| {
        core_error(
            "INVALID_TOOL_ARGUMENTS",
            "tool.interpret_intent_core.arguments",
            "tool arguments must be a JSON object",
            "Return one complete interpret_intent_core object",
        )
    })?;
    object
        .entry("runtime_requirements")
        .or_insert_with(|| serde_json::json!([]));
    object
        .entry("other_unmapped_required_capabilities")
        .or_insert_with(|| serde_json::json!([]));
    serde_json::to_string(&value).map_err(|error| {
        core_error(
            "INVALID_TOOL_ARGUMENTS",
            "tool.interpret_intent_core.arguments",
            error.to_string(),
            "Return one complete interpret_intent_core object",
        )
    })
}

fn apply_grounded_request_mode(
    input: &mut InterpretIntentCoreWireV4,
    grounded_mode: Option<IntentRequestModeV2>,
    grounded_preview: Option<bool>,
) {
    match grounded_mode {
        Some(IntentRequestModeV2::Discussion) => {
            input.request_mode = IntentRequestModeV2::Discussion;
            input.automation_kind = IntentAutomationKindV2::None;
            input.requested_outcome = IntentRequestedOutcome::Discussion;
            input.hub_channel = RequiredNullableChannelV3(None);
            input.close_policy = CloseAuthorizationV2::NotRequested;
            input.runtime_requirements.clear();
            input.other_unmapped_required_capabilities.clear();
            input.custom_detail_facets.clear();
        }
        Some(IntentRequestModeV2::Build) => {
            input.request_mode = IntentRequestModeV2::Build;
            input.requested_outcome = match grounded_preview {
                Some(true) => IntentRequestedOutcome::ValidatedPreview,
                Some(false) => IntentRequestedOutcome::WorkingDraft,
                None if input.requested_outcome == IntentRequestedOutcome::Discussion => {
                    IntentRequestedOutcome::WorkingDraft
                }
                None => input.requested_outcome,
            };
            input.response.clear();
        }
        None => {}
    }
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
    mut input: InterpretIntentCoreWireV4,
) -> Result<IntentCoreInterpretationV4, StructuredError> {
    validate_mode_outcome(&input)?;
    input.response = normalized_response(&input.response, input.request_mode)?;
    normalize_channel(&mut input.hub_channel)?;
    if input.runtime_requirements.len() > MAX_RUNTIME_REQUIREMENTS {
        return Err(core_error(
            "TOO_MANY_RUNTIME_REQUIREMENTS",
            "intent.core.runtime_requirements",
            "The interpretation contains more than four runtime requirements",
            "Use each closed runtime requirement at most once",
        ));
    }
    input.runtime_requirements = input
        .runtime_requirements
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    input.other_unmapped_required_capabilities = normalize_requirements(
        input.other_unmapped_required_capabilities,
        MAX_UNCLASSIFIED_REQUIREMENTS,
        MAX_UNCLASSIFIED_REQUIREMENT_CHARS,
        "intent.core.other_unmapped_required_capabilities",
    )?;
    input
        .other_unmapped_required_capabilities
        .retain(|value| !runtime_requirement_is_redundant(value, &input.runtime_requirements));
    if input.custom_detail_facets.len() > 3 {
        return Err(core_error(
            "TOO_MANY_RECIPE_DETAIL_FACETS",
            "intent.core.custom_detail_facets",
            "The interpretation contains more than three recipe detail facets",
            "Use only custom_copy, custom_naming, and custom_controls",
        ));
    }
    input.custom_detail_facets = input
        .custom_detail_facets
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !input.custom_detail_facets.is_empty()
        && (input.request_mode != IntentRequestModeV2::Build
            || input.automation_kind != IntentAutomationKindV2::ManagedPrivateStudyRoom)
    {
        return Err(core_error(
            "INCONSISTENT_INTENT_CORE",
            "intent.core.custom_detail_facets",
            "Recipe detail facets require a managed private study-room build",
            "Use custom detail facets only for explicit copy, naming, or control literals of that recipe",
        ));
    }
    let mut boundary_requests = BTreeSet::new();
    if [
        input.validation_gate,
        input.preview_gate,
        input.approval_gate,
    ]
    .contains(&GateDispositionWireV3::Skip)
    {
        boundary_requests.insert(IntentBoundaryRequestV2::BypassValidationPreviewApproval);
    }
    if input.live_discord_mutation == LiveDiscordMutationWireV3::MutateLiveNow {
        boundary_requests.insert(IntentBoundaryRequestV2::DirectLiveMutation);
    }
    if input.secret_disclosure == SecretDisclosureWireV3::DiscloseSecretValue {
        boundary_requests.insert(IntentBoundaryRequestV2::SecretDisclosure);
    }
    Ok(IntentCoreInterpretationV4 {
        expected_revision: input.expected_revision,
        request_mode: input.request_mode,
        automation_kind: input.automation_kind,
        requested_outcome: input.requested_outcome,
        hub_channel: input.hub_channel,
        locale: input.language,
        close_authorization: input.close_policy,
        runtime_requirements: normalize_runtime_requirements(input.runtime_requirements),
        boundary_requests: boundary_requests.into_iter().collect(),
        unclassified_requirements: input.other_unmapped_required_capabilities,
        detail_facets: input
            .custom_detail_facets
            .into_iter()
            .map(IntentRecipeDetailFacetV3::from)
            .collect(),
        response: input.response,
    })
}

fn validate_mode_outcome(input: &InterpretIntentCoreWireV4) -> Result<(), StructuredError> {
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
    if request_mode == IntentRequestModeV2::Build {
        return Ok(String::new());
    }
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(core_error(
            "EMPTY_INTENT_TEXT",
            "intent.core.response",
            "A discussion response is empty",
            "Provide one natural response to the human",
        ));
    }
    validate_text_shape(
        &normalized,
        MAX_DISCUSSION_RESPONSE_CHARS,
        true,
        false,
        "intent.core.response",
    )?;
    Ok(normalized)
}

fn normalize_runtime_requirements(values: Vec<RuntimeRequirementV3>) -> RuntimeRequirementsV2 {
    let values = values.into_iter().collect::<BTreeSet<_>>();
    RuntimeRequirementsV2 {
        persistence: if values.contains(&RuntimeRequirementV3::RestartPersistent) {
            PersistenceRequirementV2::RestartPersistent
        } else {
            PersistenceRequirementV2::None
        },
        timers: if values.contains(&RuntimeRequirementV3::DurableTimer) {
            TimerRequirementV2::Durable
        } else {
            TimerRequirementV2::None
        },
        economy: if values.contains(&RuntimeRequirementV3::PersistentEconomy) {
            EconomyRequirementV2::PersistentLedger
        } else {
            EconomyRequirementV2::None
        },
        event_time_llm: values.contains(&RuntimeRequirementV3::EventTimeLlm),
    }
}

fn runtime_requirement_is_redundant(
    value: &str,
    runtime_requirements: &[RuntimeRequirementV3],
) -> bool {
    runtime_requirements
        .iter()
        .any(|requirement| runtime_requirement_name(*requirement) == value)
}

fn runtime_requirement_name(value: RuntimeRequirementV3) -> &'static str {
    match value {
        RuntimeRequirementV3::RestartPersistent => "restart_persistent",
        RuntimeRequirementV3::DurableTimer => "durable_timer",
        RuntimeRequirementV3::PersistentEconomy => "persistent_economy",
        RuntimeRequirementV3::EventTimeLlm => "event_time_llm",
    }
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
