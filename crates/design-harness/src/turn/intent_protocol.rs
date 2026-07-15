use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{
    ExistingChannelKey, IntentLocaleV1, IntentRequestedOutcome, PrivateStudyRoomControlsProposalV1,
    PrivateStudyRoomCopyProposalV1, PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV2,
};
use crate::tools::ToolDefinition;

const ROUTE_INTENT_TURN: &str = "route_intent_turn";
pub(crate) const RESOLVE_INTENT_DECISION: &str = "resolve_intent_decision";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteIntentTurnInputV1 {
    pub expected_revision: u64,
    pub route: IntentRouteInputV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntentRouteInputV1 {
    PrivateStudyRoom {
        proposal: Box<PrivateStudyRoomProposalV2>,
    },
    TypedPlanner {
        reason: String,
        response: String,
    },
    CapabilityGap {
        capabilities: Vec<String>,
        response: String,
    },
    Reject {
        reason: String,
        response: String,
    },
    Discussion {
        response: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum IntentRouteKindV1 {
    PrivateStudyRoom,
    TypedPlanner,
    CapabilityGap,
    Reject,
    Discussion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum PrivateStudyRoomRequestedOutcomeV1 {
    WorkingDraft,
    ValidatedPreview,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PrivateStudyRoomProposalWireV1 {
    #[schemars(description = "Exact build result: working_draft or validated_preview")]
    requested_outcome: PrivateStudyRoomRequestedOutcomeV1,
    #[serde(default)]
    #[schemars(description = "Exact available channel key explicitly selected by the human")]
    hub_channel: Option<ExistingChannelKey>,
    #[serde(default)]
    locale: Option<IntentLocaleV1>,
    #[serde(default)]
    copy: PrivateStudyRoomCopyProposalV1,
    #[serde(default)]
    naming: PrivateStudyRoomNamingProposalV1,
    #[serde(default)]
    controls: PrivateStudyRoomControlsProposalV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RouteIntentTurnWireV1 {
    expected_revision: u64,
    route: IntentRouteKindV1,
    #[serde(default)]
    proposal: Option<Box<PrivateStudyRoomProposalWireV1>>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    response: Option<String>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteIntentTurnShapeProbeV1 {
    #[serde(rename = "expected_revision")]
    _expected_revision: Value,
    route: Value,
    #[serde(default)]
    #[serde(rename = "proposal")]
    _proposal: Option<Value>,
    #[serde(default)]
    #[serde(rename = "reason")]
    _reason: Option<Value>,
    #[serde(default)]
    #[serde(rename = "response")]
    _response: Option<Value>,
    #[serde(default)]
    #[serde(rename = "capabilities")]
    _capabilities: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveIntentDecisionInputV1 {
    pub expected_revision: u64,
    pub channel: ExistingChannelKey,
}

pub fn route_intent_turn_frontier() -> [ToolDefinition; 1] {
    [definition::<RouteIntentTurnWireV1>(
        ROUTE_INTENT_TURN,
        "Route this user turn exactly once. Set route to private_study_room and proposal to the exact user-facing study-room fields for the managed recipe. Set proposal.requested_outcome to exactly working_draft or validated_preview. Use the top-level discussion route when no build is requested. Otherwise set route to typed_planner with reason and response, capability_gap with capabilities and response, reject with reason and response, or discussion with response. Omit payload fields that do not belong to the selected route",
    )]
}

pub fn resolve_intent_decision_frontier() -> [ToolDefinition; 1] {
    [definition::<ResolveIntentDecisionInputV1>(
        RESOLVE_INTENT_DECISION,
        "Resolve the active existing-channel decision exactly once with the current intent revision and one available channel key",
    )]
}

pub fn parse_route_intent_turn(arguments: &str) -> Result<RouteIntentTurnInputV1, StructuredError> {
    let probe =
        serde_json::from_str::<RouteIntentTurnShapeProbeV1>(arguments).map_err(|error| {
            translate_tool_arguments_error(
                ROUTE_INTENT_TURN,
                &error,
                &schema_value::<RouteIntentTurnWireV1>(),
            )
        })?;
    if probe.route.is_object() {
        let input = serde_json::from_str::<RouteIntentTurnInputV1>(arguments).map_err(|error| {
            translate_tool_arguments_error(
                ROUTE_INTENT_TURN,
                &error,
                &schema_value::<RouteIntentTurnInputV1>(),
            )
        })?;
        return ensure_build_outcome(input);
    }
    let wire = serde_json::from_str::<RouteIntentTurnWireV1>(arguments).map_err(|error| {
        translate_tool_arguments_error(
            ROUTE_INTENT_TURN,
            &error,
            &schema_value::<RouteIntentTurnWireV1>(),
        )
    })?;
    wire.into_input()
}

pub fn parse_resolve_intent_decision(
    arguments: &str,
) -> Result<ResolveIntentDecisionInputV1, StructuredError> {
    parse(RESOLVE_INTENT_DECISION, arguments)
}

fn definition<T: JsonSchema>(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: schema_value::<T>(),
    }
}

fn schema_value<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({}))
}

fn parse<T: DeserializeOwned + JsonSchema>(
    name: &str,
    arguments: &str,
) -> Result<T, StructuredError> {
    serde_json::from_str(arguments).map_err(|error| {
        let parameters = schema_value::<T>();
        translate_tool_arguments_error(name, &error, &parameters)
    })
}

impl RouteIntentTurnWireV1 {
    fn into_input(self) -> Result<RouteIntentTurnInputV1, StructuredError> {
        self.reject_irrelevant()?;
        let Self {
            expected_revision,
            route,
            proposal,
            reason,
            response,
            capabilities,
        } = self;
        let route = match route {
            IntentRouteKindV1::PrivateStudyRoom => IntentRouteInputV1::PrivateStudyRoom {
                proposal: Box::new((*required_route_field(proposal, "proposal")?).into()),
            },
            IntentRouteKindV1::TypedPlanner => IntentRouteInputV1::TypedPlanner {
                reason: required_route_field(reason, "reason")?,
                response: required_route_field(response, "response")?,
            },
            IntentRouteKindV1::CapabilityGap => IntentRouteInputV1::CapabilityGap {
                capabilities: required_route_field(capabilities, "capabilities")?,
                response: required_route_field(response, "response")?,
            },
            IntentRouteKindV1::Reject => IntentRouteInputV1::Reject {
                reason: required_route_field(reason, "reason")?,
                response: required_route_field(response, "response")?,
            },
            IntentRouteKindV1::Discussion => IntentRouteInputV1::Discussion {
                response: required_route_field(response, "response")?,
            },
        };
        Ok(RouteIntentTurnInputV1 {
            expected_revision,
            route,
        })
    }

    fn reject_irrelevant(&self) -> Result<(), StructuredError> {
        let invalid = match self.route {
            IntentRouteKindV1::PrivateStudyRoom => [
                ("reason", self.reason.is_some()),
                ("response", self.response.is_some()),
                ("capabilities", self.capabilities.is_some()),
            ]
            .into_iter()
            .find_map(|(field, present)| present.then_some(field)),
            IntentRouteKindV1::TypedPlanner | IntentRouteKindV1::Reject => [
                ("proposal", self.proposal.is_some()),
                ("capabilities", self.capabilities.is_some()),
            ]
            .into_iter()
            .find_map(|(field, present)| present.then_some(field)),
            IntentRouteKindV1::CapabilityGap => [
                ("proposal", self.proposal.is_some()),
                ("reason", self.reason.is_some()),
            ]
            .into_iter()
            .find_map(|(field, present)| present.then_some(field)),
            IntentRouteKindV1::Discussion => [
                ("proposal", self.proposal.is_some()),
                ("reason", self.reason.is_some()),
                ("capabilities", self.capabilities.is_some()),
            ]
            .into_iter()
            .find_map(|(field, present)| present.then_some(field)),
        };
        let Some(field) = invalid else {
            return Ok(());
        };
        Err(StructuredError::new(
            "INVALID_ROUTE_PAYLOAD",
            format!("tool.{ROUTE_INTENT_TURN}.arguments.{field}"),
            format!("field {field} does not belong to the selected route"),
            "Omit payload fields for other routes",
        ))
    }
}

impl From<PrivateStudyRoomProposalWireV1> for PrivateStudyRoomProposalV2 {
    fn from(value: PrivateStudyRoomProposalWireV1) -> Self {
        Self {
            requested_outcome: match value.requested_outcome {
                PrivateStudyRoomRequestedOutcomeV1::WorkingDraft => {
                    IntentRequestedOutcome::WorkingDraft
                }
                PrivateStudyRoomRequestedOutcomeV1::ValidatedPreview => {
                    IntentRequestedOutcome::ValidatedPreview
                }
            },
            hub_channel: value.hub_channel,
            locale: value.locale,
            copy: value.copy,
            naming: value.naming,
            controls: value.controls,
        }
    }
}

fn ensure_build_outcome(
    input: RouteIntentTurnInputV1,
) -> Result<RouteIntentTurnInputV1, StructuredError> {
    if matches!(
        &input.route,
        IntentRouteInputV1::PrivateStudyRoom { proposal }
            if proposal.requested_outcome == IntentRequestedOutcome::Discussion
    ) {
        return Err(StructuredError::new(
            "INVALID_ROUTE_PAYLOAD",
            format!("tool.{ROUTE_INTENT_TURN}.arguments.proposal.requested_outcome"),
            "private_study_room requires a build outcome",
            "Use working_draft or validated_preview, or use the top-level discussion route",
        ));
    }
    Ok(input)
}

fn required_route_field<T>(value: Option<T>, field: &str) -> Result<T, StructuredError> {
    value.ok_or_else(|| {
        StructuredError::new(
            "MISSING_REQUIRED_FIELD",
            format!("tool.{ROUTE_INTENT_TURN}.arguments.{field}"),
            format!("field {field} is required for the selected route"),
            format!("Provide {field} and omit payload fields for other routes"),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;

    #[test]
    fn route_frontier_exposes_one_flat_discriminated_tool() {
        let frontier = route_intent_turn_frontier();
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0].name, ROUTE_INTENT_TURN);

        let properties = property_names(&frontier[0].parameters);
        assert_eq!(
            required_names(&frontier[0].parameters),
            BTreeSet::from(["expected_revision".to_string(), "route".to_string()])
        );
        assert_eq!(
            frontier[0].parameters.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
        assert!(properties.contains("expected_revision"));
        assert!(properties.contains("route"));
        assert!(properties.contains("proposal"));
        assert!(properties.contains("reason"));
        assert!(properties.contains("response"));
        assert!(properties.contains("capabilities"));
        assert!(!properties.contains("schema_version"));
        assert!(!properties.contains("revision"));
        assert!(!properties.contains("feature_id"));
        assert!(!properties.contains("recipe"));
        assert!(!properties.contains("source"));
        assert!(!properties.contains("actions"));
        assert!(!properties.contains("permissions"));
        assert!(!properties.contains("manifest"));
        assert!(!properties.contains("ruleset"));
        assert!(!frontier[0].parameters.to_string().contains("oneOf"));
        assert_eq!(
            enum_strings(
                frontier[0]
                    .parameters
                    .get("properties")
                    .and_then(Value::as_object)
                    .and_then(|properties| properties.get("route"))
                    .unwrap(),
                &frontier[0].parameters,
            ),
            BTreeSet::from([
                "capability_gap".to_string(),
                "discussion".to_string(),
                "private_study_room".to_string(),
                "reject".to_string(),
                "typed_planner".to_string(),
            ])
        );
        assert_eq!(
            enum_strings(
                find_property(&frontier[0].parameters, "requested_outcome").unwrap(),
                &frontier[0].parameters,
            ),
            BTreeSet::from(["validated_preview".to_string(), "working_draft".to_string(),])
        );
    }

    #[test]
    fn route_parser_accepts_every_typed_variant() {
        let private_room = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":"private_study_room","proposal":{"requested_outcome":"validated_preview"}}"#,
        )
        .unwrap();
        assert_eq!(private_room.expected_revision, 7);
        assert!(matches!(
            private_room.route,
            IntentRouteInputV1::PrivateStudyRoom { .. }
        ));

        let typed_planner = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":"typed_planner","reason":"different recipe","response":"I can design it with the typed planner."}"#,
        )
        .unwrap();
        assert!(matches!(
            typed_planner.route,
            IntentRouteInputV1::TypedPlanner { .. }
        ));

        let capability_gap = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":"capability_gap","capabilities":["creator_only_close"],"response":"Creator-only close is unavailable."}"#,
        )
        .unwrap();
        assert!(matches!(
            capability_gap.route,
            IntentRouteInputV1::CapabilityGap { .. }
        ));

        let reject = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":"reject","reason":"unsafe","response":"I cannot design that."}"#,
        )
        .unwrap();
        assert!(matches!(reject.route, IntentRouteInputV1::Reject { .. }));

        let discussion = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":"discussion","response":"Let us compare the options."}"#,
        )
        .unwrap();
        assert!(matches!(
            discussion.route,
            IntentRouteInputV1::Discussion { .. }
        ));
    }

    #[test]
    fn route_parser_keeps_the_nested_v1_input_compatible() {
        let parsed = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":{"kind":"private_study_room","proposal":{"requested_outcome":"validated_preview"}}}"#,
        )
        .unwrap();
        assert!(matches!(
            parsed.route,
            IntentRouteInputV1::PrivateStudyRoom { .. }
        ));
    }

    #[test]
    fn route_parser_rejects_duplicate_fields_before_shape_detection() {
        for arguments in [
            r#"{"expected_revision":0,"expected_revision":1,"route":"discussion","response":"ok"}"#,
            r#"{"expected_revision":0,"route":"discussion","route":"reject","response":"ok"}"#,
            r#"{"expected_revision":0,"route":"private_study_room","proposal":{"requested_outcome":"working_draft","requested_outcome":"validated_preview"}}"#,
            r#"{"expected_revision":0,"route":{"kind":"discussion","kind":"reject","response":"ok"}}"#,
        ] {
            assert_eq!(
                parse_route_intent_turn(arguments).unwrap_err().code,
                "INVALID_TOOL_ARGUMENTS"
            );
        }
    }

    #[test]
    fn private_room_discussion_outcome_is_not_a_build_route() {
        let flat = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":"private_study_room","proposal":{"requested_outcome":"discussion"}}"#,
        )
        .unwrap_err();
        assert_eq!(flat.code, "INVALID_TOOL_ARGUMENTS");

        let nested = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":{"kind":"private_study_room","proposal":{"requested_outcome":"discussion"}}}"#,
        )
        .unwrap_err();
        assert_eq!(nested.code, "INVALID_ROUTE_PAYLOAD");
        assert!(nested.location.ends_with(".proposal.requested_outcome"));
    }

    #[test]
    fn route_parser_rejects_unknown_fields_at_every_boundary() {
        let top_level = parse_route_intent_turn(
            r#"{"expected_revision":0,"schema_version":1,"route":"discussion","response":"ok"}"#,
        )
        .unwrap_err();
        assert_eq!(top_level.code, "UNKNOWN_FIELD");

        let route = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":{"kind":"discussion","response":"ok","recipe":"hidden"}}"#,
        )
        .unwrap_err();
        assert_eq!(route.code, "UNKNOWN_FIELD");

        let proposal = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":"private_study_room","proposal":{"requested_outcome":"working_draft","objective":"model summary"}}"#,
        )
        .unwrap_err();
        assert_eq!(proposal.code, "UNKNOWN_FIELD");
    }

    #[test]
    fn route_parser_translates_missing_and_invalid_arguments() {
        let missing =
            parse_route_intent_turn(r#"{"expected_revision":0,"route":"discussion"}"#).unwrap_err();
        assert_eq!(missing.code, "MISSING_REQUIRED_FIELD");
        assert!(missing.location.ends_with(".response"));

        let invalid_kind =
            parse_route_intent_turn(r#"{"expected_revision":0,"route":"recipe","response":"ok"}"#)
                .unwrap_err();
        assert_eq!(invalid_kind.code, "INVALID_TOOL_ARGUMENTS");

        let irrelevant = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":"discussion","response":"ok","reason":"extra"}"#,
        )
        .unwrap_err();
        assert_eq!(irrelevant.code, "INVALID_ROUTE_PAYLOAD");

        let syntax = parse_route_intent_turn("{").unwrap_err();
        assert_eq!(syntax.code, "INVALID_TOOL_ARGUMENTS");
    }

    #[test]
    fn decision_frontier_and_parser_are_exact_and_strict() {
        let frontier = resolve_intent_decision_frontier();
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0].name, RESOLVE_INTENT_DECISION);
        assert_eq!(
            property_names(&frontier[0].parameters),
            BTreeSet::from(["channel".to_string(), "expected_revision".to_string()])
        );

        let parsed =
            parse_resolve_intent_decision(r#"{"expected_revision":3,"channel":"study_hub"}"#)
                .unwrap();
        assert_eq!(parsed.expected_revision, 3);
        assert_eq!(parsed.channel.as_str(), "study_hub");

        let unknown = parse_resolve_intent_decision(
            r#"{"expected_revision":3,"channel":"study_hub","decision_id":"hub"}"#,
        )
        .unwrap_err();
        assert_eq!(unknown.code, "UNKNOWN_FIELD");

        let missing = parse_resolve_intent_decision(r#"{"expected_revision":3}"#).unwrap_err();
        assert_eq!(missing.code, "MISSING_REQUIRED_FIELD");
        assert!(missing.location.ends_with(".channel"));
    }

    fn property_names(value: &Value) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        collect_property_names(value, &mut names);
        names
    }

    fn required_names(value: &Value) -> BTreeSet<String> {
        value
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    }

    fn enum_strings(value: &Value, root: &Value) -> BTreeSet<String> {
        let value = value
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
            .and_then(|name| root.get("$defs").and_then(|defs| defs.get(name)))
            .unwrap_or(value);
        value
            .get("enum")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    }

    fn find_property<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        if let Some(property) = value
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|properties| properties.get(name))
        {
            return Some(property);
        }
        match value {
            Value::Array(values) => values.iter().find_map(|value| find_property(value, name)),
            Value::Object(values) => values.values().find_map(|value| find_property(value, name)),
            _ => None,
        }
    }

    fn collect_property_names(value: &Value, names: &mut BTreeSet<String>) {
        if let Some(properties) = value.get("properties").and_then(Value::as_object) {
            names.extend(properties.keys().cloned());
        }
        match value {
            Value::Array(values) => {
                for value in values {
                    collect_property_names(value, names);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    collect_property_names(value, names);
                }
            }
            _ => {}
        }
    }
}
