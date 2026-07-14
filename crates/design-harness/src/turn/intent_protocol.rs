use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;

use crate::errors::{translate_tool_arguments_error, StructuredError};
use crate::intent::{ExistingChannelKey, PrivateStudyRoomProposalV1};
use crate::tools::ToolDefinition;

const ROUTE_INTENT_TURN: &str = "route_intent_turn";
const RESOLVE_INTENT_DECISION: &str = "resolve_intent_decision";

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
        proposal: Box<PrivateStudyRoomProposalV1>,
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

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveIntentDecisionInputV1 {
    pub expected_revision: u64,
    pub channel: ExistingChannelKey,
}

pub fn route_intent_turn_frontier() -> [ToolDefinition; 1] {
    [definition::<RouteIntentTurnInputV1>(
        ROUTE_INTENT_TURN,
        "Route this user turn exactly once. Use private_study_room for a managed private study-room request and fill only user-facing proposal fields. Use typed_planner for other supported automation design, capability_gap for unsupported requested behavior, reject for disallowed requests, or discussion when no Draft change is requested",
    )]
}

pub fn resolve_intent_decision_frontier() -> [ToolDefinition; 1] {
    [definition::<ResolveIntentDecisionInputV1>(
        RESOLVE_INTENT_DECISION,
        "Resolve the active existing-channel decision exactly once with the current intent revision and one available channel key",
    )]
}

pub fn parse_route_intent_turn(arguments: &str) -> Result<RouteIntentTurnInputV1, StructuredError> {
    parse(ROUTE_INTENT_TURN, arguments)
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
        parameters: serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({})),
    }
}

fn parse<T: DeserializeOwned + JsonSchema>(
    name: &str,
    arguments: &str,
) -> Result<T, StructuredError> {
    serde_json::from_str(arguments).map_err(|error| {
        let parameters = serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({}));
        translate_tool_arguments_error(name, &error, &parameters)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;

    #[test]
    fn route_frontier_exposes_exactly_one_strict_union_tool() {
        let frontier = route_intent_turn_frontier();
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0].name, ROUTE_INTENT_TURN);

        let properties = property_names(&frontier[0].parameters);
        assert!(properties.contains("expected_revision"));
        assert!(properties.contains("route"));
        assert!(properties.contains("kind"));
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
    }

    #[test]
    fn route_parser_accepts_every_typed_variant() {
        let private_room = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":{"kind":"private_study_room","proposal":{"objective":"Create private study rooms","requested_outcome":"validated_preview"}}}"#,
        )
        .unwrap();
        assert_eq!(private_room.expected_revision, 7);
        assert!(matches!(
            private_room.route,
            IntentRouteInputV1::PrivateStudyRoom { .. }
        ));

        let typed_planner = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":{"kind":"typed_planner","reason":"different recipe","response":"I can design it with the typed planner."}}"#,
        )
        .unwrap();
        assert!(matches!(
            typed_planner.route,
            IntentRouteInputV1::TypedPlanner { .. }
        ));

        let capability_gap = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":{"kind":"capability_gap","capabilities":["creator_only_close"],"response":"Creator-only close is unavailable."}}"#,
        )
        .unwrap();
        assert!(matches!(
            capability_gap.route,
            IntentRouteInputV1::CapabilityGap { .. }
        ));

        let reject = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":{"kind":"reject","reason":"unsafe","response":"I cannot design that."}}"#,
        )
        .unwrap();
        assert!(matches!(reject.route, IntentRouteInputV1::Reject { .. }));

        let discussion = parse_route_intent_turn(
            r#"{"expected_revision":7,"route":{"kind":"discussion","response":"Let us compare the options."}}"#,
        )
        .unwrap();
        assert!(matches!(
            discussion.route,
            IntentRouteInputV1::Discussion { .. }
        ));
    }

    #[test]
    fn route_parser_rejects_unknown_fields_at_every_boundary() {
        let top_level = parse_route_intent_turn(
            r#"{"expected_revision":0,"schema_version":1,"route":{"kind":"discussion","response":"ok"}}"#,
        )
        .unwrap_err();
        assert_eq!(top_level.code, "UNKNOWN_FIELD");

        let route = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":{"kind":"discussion","response":"ok","recipe":"hidden"}}"#,
        )
        .unwrap_err();
        assert_eq!(route.code, "UNKNOWN_FIELD");

        let proposal = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":{"kind":"private_study_room","proposal":{"objective":"room","requested_outcome":"working_draft","source":"model"}}}"#,
        )
        .unwrap_err();
        assert_eq!(proposal.code, "UNKNOWN_FIELD");
    }

    #[test]
    fn route_parser_translates_missing_and_invalid_arguments() {
        let missing =
            parse_route_intent_turn(r#"{"expected_revision":0,"route":{"kind":"discussion"}}"#)
                .unwrap_err();
        assert_eq!(missing.code, "MISSING_REQUIRED_FIELD");
        assert!(missing.location.ends_with(".response"));

        let invalid_kind = parse_route_intent_turn(
            r#"{"expected_revision":0,"route":{"kind":"recipe","response":"ok"}}"#,
        )
        .unwrap_err();
        assert_eq!(invalid_kind.code, "INVALID_KIND");

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
