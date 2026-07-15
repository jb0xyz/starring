use super::intent_boundary_grounding::UnquotedGroundingLink;
use super::intent_interpretation::{
    EconomyRequirementV2, PersistenceRequirementV2, RuntimeRequirementsV2, TimerRequirementV2,
};
use super::intent_request_mode_grounding::GroundedSemanticUnit;

mod alternatives;
mod classification;
mod matching;
mod patterns;

use alternatives::{
    validate_inline_runtime_alternative, validate_inline_runtime_conflict,
    validate_runtime_alternative,
};
use classification::{
    bare_requirement_rejection, durable_timer_rejected, durable_timer_required,
    event_context_barrier, event_time_llm_rejected, event_time_llm_required, persistence_rejected,
    persistence_required, persistent_economy_rejected, persistent_economy_required,
    positive_runtime_axes,
};
use matching::has_any;
use patterns::EVENT_TIME_MARKERS;

#[cfg(test)]
pub(super) fn requirement_action_occurrence_scans(text: &str) -> usize {
    matching::requirement_action_occurrence_scans(text)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeRequirementAxis {
    Persistence,
    Timers,
    Economy,
    EventTimeLlm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeGroundingAmbiguity {
    Conflict,
    Alternative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RuntimeGroundingError {
    pub(super) axis: RuntimeRequirementAxis,
    pub(super) ambiguity: RuntimeGroundingAmbiguity,
}

#[derive(Default)]
struct AxisEvidence {
    positive: bool,
    negative: bool,
}

impl AxisEvidence {
    fn observe(&mut self, positive: bool, negative: bool) {
        if negative {
            self.negative = true;
        } else if positive {
            self.positive = true;
        }
    }

    fn required(&self, axis: RuntimeRequirementAxis) -> Result<bool, RuntimeGroundingError> {
        if self.positive && self.negative {
            return Err(RuntimeGroundingError {
                axis,
                ambiguity: RuntimeGroundingAmbiguity::Conflict,
            });
        }
        Ok(self.positive)
    }

    fn reject_prior_positive(&mut self) {
        if self.positive {
            self.positive = false;
            self.negative = true;
        }
    }
}

pub(crate) fn ground_runtime_requirements(
    active_semantic_units: &[GroundedSemanticUnit],
) -> Result<RuntimeRequirementsV2, RuntimeGroundingError> {
    let mut persistence = AxisEvidence::default();
    let mut timers = AxisEvidence::default();
    let mut economy = AxisEvidence::default();
    let mut event_time_llm = AxisEvidence::default();
    let mut previous: Option<&GroundedSemanticUnit> = None;
    let mut event_scope = false;
    for unit in active_semantic_units {
        if unit.link == UnquotedGroundingLink::Detached {
            event_scope = false;
        }
        if unit.link == UnquotedGroundingLink::Alternative
            || (unit.link == UnquotedGroundingLink::Additive && unit.text.starts_with("otherwise "))
        {
            if let Some(previous) = previous {
                if unit.authoritative || previous.authoritative {
                    validate_runtime_alternative(&previous.text, &unit.text, event_scope)?;
                }
            }
        }
        if !unit.authoritative {
            event_scope = false;
            previous = Some(unit);
            continue;
        }
        validate_inline_runtime_alternative(&unit.text)?;
        validate_inline_runtime_conflict(&unit.text)?;
        if bare_requirement_rejection(&unit.text) {
            if let Some(previous) = previous.filter(|previous| previous.authoritative) {
                for axis in positive_runtime_axes(&previous.text) {
                    match axis {
                        RuntimeRequirementAxis::Persistence => persistence.reject_prior_positive(),
                        RuntimeRequirementAxis::Timers => timers.reject_prior_positive(),
                        RuntimeRequirementAxis::Economy => economy.reject_prior_positive(),
                        RuntimeRequirementAxis::EventTimeLlm => {
                            event_time_llm.reject_prior_positive()
                        }
                    }
                }
            }
        }
        let inherited_event_scope = unit.link == UnquotedGroundingLink::Additive && event_scope;
        persistence.observe(
            persistence_required(&unit.text),
            persistence_rejected(&unit.text),
        );
        timers.observe(
            durable_timer_required(&unit.text),
            durable_timer_rejected(&unit.text),
        );
        economy.observe(
            persistent_economy_required(&unit.text),
            persistent_economy_rejected(&unit.text),
        );
        event_time_llm.observe(
            event_time_llm_required(&unit.text, inherited_event_scope),
            event_time_llm_rejected(&unit.text, inherited_event_scope),
        );
        let explicit_event_scope = has_any(&unit.text, EVENT_TIME_MARKERS);
        event_scope = match unit.link {
            UnquotedGroundingLink::Detached => explicit_event_scope,
            UnquotedGroundingLink::Additive => event_scope || explicit_event_scope,
            UnquotedGroundingLink::Alternative => false,
        } && !event_context_barrier(&unit.text);
        previous = Some(unit);
    }
    Ok(RuntimeRequirementsV2 {
        persistence: if persistence.required(RuntimeRequirementAxis::Persistence)? {
            PersistenceRequirementV2::RestartPersistent
        } else {
            PersistenceRequirementV2::None
        },
        timers: if timers.required(RuntimeRequirementAxis::Timers)? {
            TimerRequirementV2::Durable
        } else {
            TimerRequirementV2::None
        },
        economy: if economy.required(RuntimeRequirementAxis::Economy)? {
            EconomyRequirementV2::PersistentLedger
        } else {
            EconomyRequirementV2::None
        },
        event_time_llm: event_time_llm.required(RuntimeRequirementAxis::EventTimeLlm)?,
    })
}
