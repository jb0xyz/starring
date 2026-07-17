use super::intent_boundary_grounding::{
    analyze_safety_boundaries, unquoted_grounding_text, UnquotedGroundingLink,
};
use super::intent_interpretation::IntentRequestModeV2;

mod closed_axes;
mod directives;
mod lexical;
mod patterns;
mod semantic_authority;

pub(in crate::turn) use closed_axes::grounded_closed_axis_restatement;
pub(super) use closed_axes::{ClosedAxisGroundingError, GroundedClosedAxes};

pub(super) struct GroundedRequestControls {
    pub(super) mode: Option<IntentRequestModeV2>,
    pub(super) preview: Option<bool>,
    pub(super) active_semantic_units: Option<Vec<GroundedSemanticUnit>>,
    pub(super) closed_axes: closed_axes::GroundedClosedAxes,
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

pub(crate) fn grounded_request_mode(human: &str) -> Option<IntentRequestModeV2> {
    grounded_request_controls(human)
        .mode
        .or_else(|| safety_boundary_request_mode(human))
}

pub(super) fn safety_boundary_request_mode(human: &str) -> Option<IntentRequestModeV2> {
    (!analyze_safety_boundaries(human).requests().is_empty())
        .then_some(IntentRequestModeV2::Build)
}

#[cfg(test)]
pub(super) fn grounded_preview_preference(human: &str) -> Option<bool> {
    grounded_request_controls(human).preview
}
