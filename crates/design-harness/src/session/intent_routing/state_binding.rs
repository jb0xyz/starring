use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::StructuredError;
use crate::intent::{IntentWorkspaceV1, MissingDecision};

use super::decision::IntentRouteDecisionV2;
use super::evidence::IntentRecipeEvidenceV3;
use super::state::intent_error;

const AWAITING_DECISION_BINDING_DOMAIN_V3: &[u8] =
    b"starring.intent.stage_binding.awaiting_decision.v3\0";
const PREVIEW_READY_BINDING_DOMAIN_V3: &[u8] = b"starring.intent.stage_binding.preview_ready.v3\0";

pub(super) struct AwaitingDecisionBindingInputV3<'a> {
    pub(super) protocol_version: u16,
    pub(super) context_fingerprint: &'a str,
    pub(super) root_draft_revision: u64,
    pub(super) workspace: &'a IntentWorkspaceV1,
    pub(super) active_decision: &'a MissingDecision,
    pub(super) route_decision: &'a IntentRouteDecisionV2,
    pub(super) recipe_evidence: &'a IntentRecipeEvidenceV3,
}

pub(super) struct PreviewReadyBindingInputV3<'a> {
    pub(super) protocol_version: u16,
    pub(super) context_fingerprint: &'a str,
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
    pub(super) recipe_evidence: &'a IntentRecipeEvidenceV3,
}

pub(super) fn awaiting_decision_binding_digest_v3(
    input: AwaitingDecisionBindingInputV3<'_>,
) -> Result<String, StructuredError> {
    let projection = AwaitingDecisionBindingProjectionV3 {
        protocol_version: input.protocol_version,
        context_fingerprint: input.context_fingerprint,
        route_decision_adjudication_digest: input.route_decision.adjudication_digest(),
        recipe_evidence: input.recipe_evidence,
        root_draft_revision: input.root_draft_revision,
        workspace: input.workspace,
        active_decision: input.active_decision,
    };
    digest_binding(AWAITING_DECISION_BINDING_DOMAIN_V3, &projection)
}

pub(super) fn preview_ready_binding_digest_v3(
    input: PreviewReadyBindingInputV3<'_>,
) -> Result<String, StructuredError> {
    let projection = PreviewReadyBindingProjectionV3 {
        protocol_version: input.protocol_version,
        context_fingerprint: input.context_fingerprint,
        route_decision_adjudication_digest: input.route_decision.adjudication_digest(),
        recipe_evidence: input.recipe_evidence,
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
    digest_binding(PREVIEW_READY_BINDING_DOMAIN_V3, &projection)
}

#[derive(Serialize)]
struct AwaitingDecisionBindingProjectionV3<'a> {
    protocol_version: u16,
    context_fingerprint: &'a str,
    route_decision_adjudication_digest: &'a str,
    recipe_evidence: &'a IntentRecipeEvidenceV3,
    root_draft_revision: u64,
    workspace: &'a IntentWorkspaceV1,
    active_decision: &'a MissingDecision,
}

#[derive(Serialize)]
struct PreviewReadyBindingProjectionV3<'a> {
    protocol_version: u16,
    context_fingerprint: &'a str,
    route_decision_adjudication_digest: &'a str,
    recipe_evidence: &'a IntentRecipeEvidenceV3,
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
