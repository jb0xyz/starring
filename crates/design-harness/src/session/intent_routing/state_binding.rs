use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::StructuredError;
use crate::intent::{IntentWorkspaceV1, MissingDecision};

use super::decision::IntentRouteDecisionV2;
use super::state::intent_error;

const AWAITING_DECISION_BINDING_DOMAIN_V2: &[u8] =
    b"starring.intent.stage_binding.awaiting_decision.v2\0";
const PREVIEW_READY_BINDING_DOMAIN_V2: &[u8] = b"starring.intent.stage_binding.preview_ready.v2\0";

pub(super) struct AwaitingDecisionBindingInputV2<'a> {
    pub(super) root_draft_revision: u64,
    pub(super) workspace: &'a IntentWorkspaceV1,
    pub(super) active_decision: &'a MissingDecision,
    pub(super) route_decision: &'a IntentRouteDecisionV2,
}

pub(super) struct PreviewReadyBindingInputV2<'a> {
    pub(super) root_draft_revision: u64,
    pub(super) workspace: &'a IntentWorkspaceV1,
    pub(super) intent_revision: u64,
    pub(super) candidate_revision: u64,
    pub(super) input_intent_hash: &'a str,
    pub(super) semantic_intent_hash: &'a str,
    pub(super) compiled_plan_hash: &'a str,
    pub(super) external_channel_bindings: &'a [String],
    pub(super) compiled_operations: usize,
    pub(super) route_decision: &'a IntentRouteDecisionV2,
}

pub(super) fn awaiting_decision_binding_digest_v2(
    input: AwaitingDecisionBindingInputV2<'_>,
) -> Result<String, StructuredError> {
    let projection = AwaitingDecisionBindingProjectionV2 {
        route_decision_adjudication_digest: input.route_decision.adjudication_digest(),
        root_draft_revision: input.root_draft_revision,
        workspace: input.workspace,
        active_decision: input.active_decision,
    };
    digest_binding(AWAITING_DECISION_BINDING_DOMAIN_V2, &projection)
}

pub(super) fn preview_ready_binding_digest_v2(
    input: PreviewReadyBindingInputV2<'_>,
) -> Result<String, StructuredError> {
    let projection = PreviewReadyBindingProjectionV2 {
        route_decision_adjudication_digest: input.route_decision.adjudication_digest(),
        root_draft_revision: input.root_draft_revision,
        workspace: input.workspace,
        intent_revision: input.intent_revision,
        candidate_revision: input.candidate_revision,
        input_intent_hash: input.input_intent_hash,
        semantic_intent_hash: input.semantic_intent_hash,
        compiled_plan_hash: input.compiled_plan_hash,
        external_channel_bindings: input.external_channel_bindings,
        compiled_operations: input.compiled_operations,
    };
    digest_binding(PREVIEW_READY_BINDING_DOMAIN_V2, &projection)
}

#[derive(Serialize)]
struct AwaitingDecisionBindingProjectionV2<'a> {
    route_decision_adjudication_digest: &'a str,
    root_draft_revision: u64,
    workspace: &'a IntentWorkspaceV1,
    active_decision: &'a MissingDecision,
}

#[derive(Serialize)]
struct PreviewReadyBindingProjectionV2<'a> {
    route_decision_adjudication_digest: &'a str,
    root_draft_revision: u64,
    workspace: &'a IntentWorkspaceV1,
    intent_revision: u64,
    candidate_revision: u64,
    input_intent_hash: &'a str,
    semantic_intent_hash: &'a str,
    compiled_plan_hash: &'a str,
    external_channel_bindings: &'a [String],
    compiled_operations: usize,
}

fn digest_binding(domain: &[u8], projection: &impl Serialize) -> Result<String, StructuredError> {
    let value = serde_json::to_value(projection).map_err(binding_serialization_error)?;
    let canonical = canonicalize_json(value);
    let bytes = serde_json::to_vec(&canonical).map_err(binding_serialization_error)?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        value => value,
    }
}

fn binding_serialization_error(error: serde_json::Error) -> StructuredError {
    intent_error(
        "INTENT_STAGE_BINDING_SERIALIZATION_FAILED",
        "intent.session.stage_binding",
        "The intent stage binding could not be serialized deterministically",
        error.to_string(),
    )
}
