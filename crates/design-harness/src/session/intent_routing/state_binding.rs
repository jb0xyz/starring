use serde::Serialize;

use crate::errors::StructuredError;
use crate::intent::identity::{canonical_json_digest, IdentityErrorSpec};
use crate::intent::{IntentWorkspaceV2, MissingDecision};

use super::decision::IntentRouteDecisionV2;
use super::evidence::IntentRecipeEvidenceV4;
use super::request_evidence::IntentRequestEvidenceChainV1;

const AWAITING_DECISION_BINDING_DOMAIN_V4: &[u8] =
    b"starring.intent.stage_binding.awaiting_decision.v4\0";
const PREVIEW_READY_BINDING_DOMAIN_V4: &[u8] = b"starring.intent.stage_binding.preview_ready.v4\0";

pub(super) struct AwaitingDecisionBindingInputV4<'a> {
    pub(super) protocol_version: u16,
    pub(super) context_fingerprint: &'a str,
    pub(super) root_draft_revision: u64,
    pub(super) root_draft_hash: &'a str,
    pub(super) workspace: &'a IntentWorkspaceV2,
    pub(super) active_decision: &'a MissingDecision,
    pub(super) request_evidence: &'a IntentRequestEvidenceChainV1,
    pub(super) route_decision: &'a IntentRouteDecisionV2,
    pub(super) recipe_evidence: &'a IntentRecipeEvidenceV4,
}

pub(super) struct PreviewReadyBindingInputV4<'a> {
    pub(super) protocol_version: u16,
    pub(super) context_fingerprint: &'a str,
    pub(super) root_draft_revision: u64,
    pub(super) workspace: &'a IntentWorkspaceV2,
    pub(super) identity_revision: u16,
    pub(super) intent_revision: u64,
    pub(super) candidate_revision: u64,
    pub(super) compiler_input_hash: &'a str,
    pub(super) semantic_intent_hash: &'a str,
    pub(super) compiled_plan_hash: &'a str,
    pub(super) candidate_ruleset_hash: &'a str,
    pub(super) candidate_draft_hash: &'a str,
    pub(super) external_channel_bindings: &'a [String],
    pub(super) compiled_operations: usize,
    pub(super) request_evidence: &'a IntentRequestEvidenceChainV1,
    pub(super) route_decision: &'a IntentRouteDecisionV2,
    pub(super) recipe_evidence: &'a IntentRecipeEvidenceV4,
}

pub(super) fn awaiting_decision_binding_digest_v4(
    input: AwaitingDecisionBindingInputV4<'_>,
) -> Result<String, StructuredError> {
    let projection = AwaitingDecisionBindingProjectionV4 {
        protocol_version: input.protocol_version,
        context_fingerprint: input.context_fingerprint,
        route_decision_adjudication_digest: input.route_decision.adjudication_digest(),
        recipe_evidence: input.recipe_evidence,
        root_draft_revision: input.root_draft_revision,
        root_draft_hash: input.root_draft_hash,
        workspace: input.workspace,
        active_decision: input.active_decision,
        request_evidence: input.request_evidence,
    };
    digest_binding(AWAITING_DECISION_BINDING_DOMAIN_V4, &projection)
}

pub(super) fn preview_ready_binding_digest_v4(
    input: PreviewReadyBindingInputV4<'_>,
) -> Result<String, StructuredError> {
    let projection = PreviewReadyBindingProjectionV4 {
        protocol_version: input.protocol_version,
        context_fingerprint: input.context_fingerprint,
        route_decision_adjudication_digest: input.route_decision.adjudication_digest(),
        recipe_evidence: input.recipe_evidence,
        root_draft_revision: input.root_draft_revision,
        workspace: input.workspace,
        identity_revision: input.identity_revision,
        intent_revision: input.intent_revision,
        candidate_revision: input.candidate_revision,
        compiler_input_hash: input.compiler_input_hash,
        semantic_intent_hash: input.semantic_intent_hash,
        compiled_plan_hash: input.compiled_plan_hash,
        candidate_ruleset_hash: input.candidate_ruleset_hash,
        candidate_draft_hash: input.candidate_draft_hash,
        external_channel_bindings: input.external_channel_bindings,
        compiled_operations: input.compiled_operations,
        request_evidence: input.request_evidence,
    };
    digest_binding(PREVIEW_READY_BINDING_DOMAIN_V4, &projection)
}

#[derive(Serialize)]
struct AwaitingDecisionBindingProjectionV4<'a> {
    protocol_version: u16,
    context_fingerprint: &'a str,
    route_decision_adjudication_digest: &'a str,
    recipe_evidence: &'a IntentRecipeEvidenceV4,
    root_draft_revision: u64,
    root_draft_hash: &'a str,
    workspace: &'a IntentWorkspaceV2,
    active_decision: &'a MissingDecision,
    request_evidence: &'a IntentRequestEvidenceChainV1,
}

#[derive(Serialize)]
struct PreviewReadyBindingProjectionV4<'a> {
    protocol_version: u16,
    context_fingerprint: &'a str,
    route_decision_adjudication_digest: &'a str,
    recipe_evidence: &'a IntentRecipeEvidenceV4,
    root_draft_revision: u64,
    workspace: &'a IntentWorkspaceV2,
    identity_revision: u16,
    intent_revision: u64,
    candidate_revision: u64,
    compiler_input_hash: &'a str,
    semantic_intent_hash: &'a str,
    compiled_plan_hash: &'a str,
    candidate_ruleset_hash: &'a str,
    candidate_draft_hash: &'a str,
    external_channel_bindings: &'a [String],
    compiled_operations: usize,
    request_evidence: &'a IntentRequestEvidenceChainV1,
}

fn digest_binding(domain: &[u8], projection: &impl Serialize) -> Result<String, StructuredError> {
    canonical_json_digest(
        domain,
        projection,
        IdentityErrorSpec::new(
            "INTENT_STAGE_BINDING_SERIALIZATION_FAILED",
            "intent.session.stage_binding",
            "The intent stage binding could not be serialized deterministically",
        ),
    )
}
