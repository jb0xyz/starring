use super::intent_boundary_grounding::{unquoted_grounding_text, UnquotedGroundingLink};
use super::intent_interpretation::IntentRequestModeV2;

mod directives;
mod lexical;
mod patterns;
mod semantic_authority;

pub(super) struct GroundedRequestControls {
    pub(super) mode: Option<IntentRequestModeV2>,
    pub(super) preview: Option<bool>,
    pub(super) active_semantic_units: Option<Vec<GroundedSemanticUnit>>,
}

pub(super) struct GroundedSemanticUnit {
    pub(super) text: String,
    pub(super) authoritative: bool,
    pub(super) link: UnquotedGroundingLink,
    pub(super) operative_antecedent: bool,
}

pub(super) fn grounded_request_controls(human: &str) -> GroundedRequestControls {
    semantic_authority::grounded_request_controls(human)
}

#[cfg(test)]
pub(super) fn grounded_request_mode(human: &str) -> Option<IntentRequestModeV2> {
    grounded_request_controls(human).mode
}

#[cfg(test)]
pub(super) fn grounded_preview_preference(human: &str) -> Option<bool> {
    grounded_request_controls(human).preview
}
