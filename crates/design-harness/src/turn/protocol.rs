use resource_resolution::ResourceBindingMap;
use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

use crate::draft::{Draft, DraftSummary};
use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::tools::ToolDefinition;

use super::{
    plan_input::{self, PlanOutlineItem, TurnPlanSubmission},
    scope::ScopeRequirement,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TurnIntent {
    Build,
    Modify,
    Brainstorm,
    Inspect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestedOutcome {
    Discussion,
    DraftUpdate,
    ValidatedPreview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SimulationProfile {
    None,
    StudyRoom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockingDecision {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TurnVerification {
    pub validate: bool,
    pub simulation: SimulationProfile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TurnBrief {
    pub intent: TurnIntent,
    pub objective: String,
    pub requested_outcome: RequestedOutcome,
    pub requirements: Vec<ScopeRequirement>,
    pub assumptions: Vec<String>,
    pub blocking_decisions: Vec<BlockingDecision>,
    pub verification: TurnVerification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FinishTurnKind {
    NeedsInput,
    Progressed,
    #[serde(alias = "success")]
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinishTurn {
    pub kind: FinishTurnKind,
    pub message: String,
    #[serde(default)]
    pub question: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DraftPreview {
    pub revision: u64,
    pub draft: DraftSummary,
    pub ruleset: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdaptivePhase {
    Assess,
    Build,
    Verify,
    Simulate,
    Preview,
    Reply,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveTurnState {
    pub phase: AdaptivePhase,
    pub brief: Option<TurnBrief>,
    pub scoped_revision: Option<u64>,
    pub previewed_revision: Option<u64>,
}

impl Default for AdaptiveTurnState {
    fn default() -> Self {
        Self {
            phase: AdaptivePhase::Assess,
            brief: None,
            scoped_revision: None,
            previewed_revision: None,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetTurnBriefInput {
    intent: TurnIntent,
    objective: String,
    requested_outcome: RequestedOutcome,
    assumptions: Vec<String>,
    validate: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetTurnPlanInput {
    requirements: Vec<ScopeRequirement>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TurnStrategyInput {
    AdditivePlan,
    EditExisting,
    Brainstorm,
    Inspect,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlannedSetTurnBriefInput {
    strategy: TurnStrategyInput,
    objective: String,
    requested_outcome: RequestedOutcome,
    assumptions: Vec<String>,
    validate: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySetTurnBriefInput {
    intent: TurnIntent,
    objective: String,
    requested_outcome: RequestedOutcome,
    assumptions: Vec<String>,
    validate: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CompatibleSetTurnBriefInput {
    Current(PlannedSetTurnBriefInput),
    Legacy(LegacySetTurnBriefInput),
}

pub fn control_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        definition::<SetTurnBriefInput>(
            "set_turn_brief",
            "Classify the current user turn with a concise objective and verification plan",
        ),
        definition::<SetTurnPlanInput>(
            "set_turn_plan",
            "Declare exact ordered Draft requirements for additive work in the current build or modify-labeled turn",
        ),
        definition::<EmptyInput>(
            "check_turn_scope",
            "Check the current Draft against the active turn requirements",
        ),
        definition::<EmptyInput>(
            "render_preview",
            "Render the current validated Draft for human review",
        ),
        definition::<FinishTurn>(
            "finish_turn",
            "Finish the human turn with kind needs_input, progressed, or ready; include question only for needs_input",
        ),
    ]
}

pub(crate) fn planned_control_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        definition::<PlannedSetTurnBriefInput>(
            "set_turn_brief",
            "Choose additive_plan for every request that adds a new panel, button, modal, rule, or action, including additions to an existing Draft. Choose edit_existing only for updating or removing a target that already exists. Choose brainstorm or inspect only when no Draft mutation is requested. Record a concise objective and verification choice",
        ),
        ToolDefinition {
            name: "set_turn_plan".to_string(),
            description: "Declare the complete ordered additive outline using exactly {\"steps\":[{\"op\":\"post_panel\",\"owner\":\"submit_room\",\"goal\":\"post panel key join_panel in the created channel with its complete embedded button list\"}]}. Include only missing mutations and omit existing objects or actions mentioned only to preserve them. Every step must include owner: draft for panel, modal, and rule; its panel key for button; its parent rule key for every action. Never use a role, channel, modal, action target, or resource key as an action owner. A post_panel step contains its complete embedded buttons in its later packet; never add separate button steps for them. The button op is only for a persistent top-level panel declared with panel. Every op must be one exact string from the schema enum; there is no generic action op. Never send type or action fields, a nested op object, or typed action arguments here; fill_turn_plan_packet receives typed values later. Use one step and literal-value goal per new requested declaration or action. A rule is trigger-only and every action is separate. Put every new top-level button immediately after its panel and every action immediately after its rule".to_string(),
            parameters: plan_input::outline_schema(),
        },
        definition::<EmptyInput>(
            "check_turn_scope",
            "Check the current Draft against the active turn requirements",
        ),
        definition::<EmptyInput>(
            "render_preview",
            "Render the current validated Draft for human review",
        ),
        definition::<FinishTurn>(
            "finish_turn",
            "Finish the human turn with kind needs_input, progressed, or ready; include question only for needs_input",
        ),
    ]
}

pub fn parse_turn_brief(arguments: &str) -> Result<TurnBrief, StructuredError> {
    parse::<SetTurnBriefInput>("set_turn_brief", arguments).map(|input| TurnBrief {
        intent: input.intent,
        objective: input.objective,
        requested_outcome: input.requested_outcome,
        requirements: Vec::new(),
        assumptions: input.assumptions,
        blocking_decisions: Vec::new(),
        verification: TurnVerification {
            validate: input.validate,
            simulation: SimulationProfile::None,
        },
    })
}

pub(crate) fn parse_planned_turn_brief(arguments: &str) -> Result<TurnBrief, StructuredError> {
    let parameters = definition::<PlannedSetTurnBriefInput>("set_turn_brief", "").parameters;
    let input = serde_json::from_str::<CompatibleSetTurnBriefInput>(arguments)
        .map_err(|error| translate_tool_arguments_error("set_turn_brief", &error, &parameters))?;
    let (intent, objective, requested_outcome, assumptions, validate) = match input {
        CompatibleSetTurnBriefInput::Current(input) => {
            let intent = match input.strategy {
                TurnStrategyInput::AdditivePlan => TurnIntent::Build,
                TurnStrategyInput::EditExisting => TurnIntent::Modify,
                TurnStrategyInput::Brainstorm => TurnIntent::Brainstorm,
                TurnStrategyInput::Inspect => TurnIntent::Inspect,
            };
            (
                intent,
                input.objective,
                input.requested_outcome,
                input.assumptions,
                input.validate,
            )
        }
        CompatibleSetTurnBriefInput::Legacy(input) => (
            input.intent,
            input.objective,
            input.requested_outcome,
            input.assumptions,
            input.validate,
        ),
    };
    Ok(TurnBrief {
        intent,
        objective,
        requested_outcome,
        requirements: Vec::new(),
        assumptions,
        blocking_decisions: Vec::new(),
        verification: TurnVerification {
            validate,
            simulation: SimulationProfile::None,
        },
    })
}

pub(crate) fn parse_turn_plan(arguments: &str) -> Result<TurnPlanSubmission, StructuredError> {
    if let Ok(input) = serde_json::from_str::<SetTurnPlanInput>(arguments) {
        return Ok(TurnPlanSubmission::Complete(input.requirements));
    }
    plan_input::parse_submission(arguments)
}

pub(crate) fn plan_packet_definition(
    draft: &Draft,
    requirements: &[ScopeRequirement],
    items: &[PlanOutlineItem],
) -> ToolDefinition {
    plan_input::packet_definition_for_state(draft, requirements, items)
}

pub(crate) fn plan_review_definition(
    draft: &Draft,
    requirements: &[ScopeRequirement],
) -> ToolDefinition {
    plan_input::review_definition(draft, requirements)
}

pub(crate) fn parse_turn_plan_review(
    requirements: &[ScopeRequirement],
    arguments: &str,
) -> Result<(), StructuredError> {
    plan_input::parse_review(requirements, arguments)
}

pub(crate) fn parse_turn_plan_review_oracle(
    requirements: &[ScopeRequirement],
    arguments: &str,
) -> Result<(), StructuredError> {
    plan_input::parse_review_oracle(requirements, arguments)
}

pub(crate) fn parse_turn_plan_packet_scoped(
    draft: &Draft,
    requirements: &[ScopeRequirement],
    items: &[PlanOutlineItem],
    arguments: &str,
) -> Result<Vec<ScopeRequirement>, plan_input::PlanPacketFailure> {
    plan_input::parse_packet_for_state_scoped(draft, requirements, items, arguments)
}

pub fn parse_finish_turn(arguments: &str) -> Result<FinishTurn, StructuredError> {
    parse("finish_turn", arguments)
}

pub fn parse_empty_control(name: &str, arguments: &str) -> Result<(), StructuredError> {
    parse::<EmptyInput>(name, arguments).map(|_| ())
}

pub fn render_preview(draft: &Draft) -> DraftPreview {
    DraftPreview {
        revision: draft.draft_revision,
        draft: draft.summary(),
        ruleset: serde_json::to_value(&draft.ruleset).unwrap_or(Value::Null),
    }
}

pub(crate) fn render_preview_with_bindings(
    draft: &Draft,
    bindings: &ResourceBindingMap,
) -> Result<DraftPreview, StructuredError> {
    let ruleset = serde_json::to_value(&draft.ruleset).map_err(|error| {
        StructuredError::new(
            "PREVIEW_SERIALIZATION_FAILED",
            "draft.preview",
            format!("The candidate Draft could not be serialized: {error}"),
            "Keep the canonical Draft unchanged and report the candidate serialization failure",
        )
    })?;
    Ok(DraftPreview {
        revision: draft.draft_revision,
        draft: draft.summary_with_bindings(bindings),
        ruleset,
    })
}

fn definition<T: JsonSchema>(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({})),
    }
}

fn parse<T: DeserializeOwned + JsonSchema>(
    name: &str,
    arguments: &str,
) -> Result<T, StructuredError> {
    serde_json::from_str(arguments).map_err(|error| {
        let parameters = control_tool_definitions()
            .into_iter()
            .find(|definition| definition.name == name)
            .map(|definition| definition.parameters)
            .unwrap_or_else(|| json!({}));
        translate_tool_arguments_error(name, &error, &parameters)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;

    #[test]
    fn turn_brief_rejects_legacy_requirement_and_verification_shapes() {
        let requirements = r#"{"intent":"build","objective":"room","requested_outcome":"draft_update","requirements":[],"assumptions":[],"validate":false}"#;
        assert!(parse_turn_brief(requirements).is_err());

        let verification = r#"{"intent":"build","objective":"room","requested_outcome":"draft_update","assumptions":[],"verification":{"validate":false}}"#;
        assert!(parse_turn_brief(verification).is_err());
    }

    #[test]
    fn control_tool_schemas_parse_strict_inputs() {
        let parsed = parse_turn_brief(
            r#"{"intent":"brainstorm","objective":"game","requested_outcome":"discussion","assumptions":[],"validate":false}"#,
        )
        .unwrap();
        assert_eq!(parsed.intent, TurnIntent::Brainstorm);
        assert!(parsed.requirements.is_empty());
        assert!(parsed.blocking_decisions.is_empty());
        assert!(!parsed.verification.validate);
        assert_eq!(parsed.verification.simulation, SimulationProfile::None);
        assert!(parse_turn_brief(
            r#"{"strategy":"brainstorm","objective":"game","requested_outcome":"discussion","assumptions":[],"validate":false}"#
        )
        .is_err());
        let planned = parse_planned_turn_brief(
            r#"{"strategy":"additive_plan","objective":"room","requested_outcome":"draft_update","assumptions":[],"validate":false}"#,
        )
        .unwrap();
        assert_eq!(planned.intent, TurnIntent::Build);
        assert!(parse_planned_turn_brief(
            r#"{"intent":"build","objective":"room","requested_outcome":"draft_update","assumptions":[],"validate":false}"#
        )
        .is_ok());
        assert!(parse_turn_brief(
            r#"{"intent":"brainstorm","objective":"game","requested_outcome":"discussion","assumptions":[],"validate":false,"simulation":"study_room"}"#
        )
        .is_err());
        let schema = control_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "set_turn_brief")
            .unwrap()
            .parameters;
        assert!(schema.pointer("/properties/strategy").is_none());
        assert!(schema.pointer("/properties/intent").is_some());
        assert!(schema.pointer("/properties/validate").is_some());
        assert!(schema.pointer("/properties/simulation").is_none());
        let planned_schema = planned_control_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "set_turn_brief")
            .unwrap()
            .parameters;
        assert!(planned_schema.pointer("/properties/strategy").is_some());
        assert!(planned_schema.pointer("/properties/intent").is_none());
        let plan_definition = planned_control_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "set_turn_plan")
            .unwrap();
        assert!(plan_definition
            .description
            .contains("{\"steps\":[{\"op\":\"post_panel\""));
        assert!(plan_definition
            .description
            .contains("Never send type or action fields"));
        assert!(plan_definition
            .description
            .contains("Include only missing mutations"));
        assert!(plan_definition
            .description
            .contains("its parent rule key for every action"));
        let adaptive_plan = control_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "set_turn_plan")
            .unwrap();
        assert!(adaptive_plan
            .parameters
            .pointer("/properties/requirements")
            .is_some());
        assert!(adaptive_plan
            .parameters
            .pointer("/properties/steps")
            .is_none());
        let plan = parse_turn_plan(
            r#"{"requirements":[{"kind":"no_unresolved_references","id":"refs"}]}"#,
        )
        .unwrap();
        let TurnPlanSubmission::Complete(requirements) = plan else {
            panic!("expected complete plan")
        };
        assert_eq!(requirements.len(), 1);
        assert!(parse_turn_plan(r#"{"requirements":[],"extra":true}"#).is_err());
        assert!(parse_finish_turn(
            r#"{"kind":"needs_input","message":"Choose a genre","question":"Which genre?"}"#
        )
        .is_ok());
        assert_eq!(
            parse_finish_turn(r#"{"kind":"ready","message":"Ready"}"#)
                .unwrap()
                .kind,
            FinishTurnKind::Ready
        );
        assert_eq!(
            parse_finish_turn(r#"{"kind":"success","message":"Ready"}"#)
                .unwrap()
                .kind,
            FinishTurnKind::Ready
        );
        assert!(parse_finish_turn(r#"{"kind":"ready","message":"Ready","changes":[]}"#).is_err());
        assert!(parse_finish_turn(r#"{"kind":"done","message":"Ready"}"#).is_err());
        let finish_schema = control_tool_definitions()
            .into_iter()
            .find(|tool| tool.name == "finish_turn")
            .unwrap()
            .parameters;
        let properties = finish_schema["properties"].as_object().unwrap();
        assert_eq!(
            properties.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "kind".to_string(),
                "message".to_string(),
                "question".to_string()
            ])
        );
        assert_eq!(finish_schema["required"], json!(["kind", "message"]));
        assert_eq!(
            finish_schema["$defs"]["FinishTurnKind"]["enum"],
            json!(["needs_input", "progressed", "ready"])
        );
        assert_eq!(control_tool_definitions().len(), 5);
        assert_eq!(planned_control_tool_definitions().len(), 5);
    }
}
