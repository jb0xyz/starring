use crate::turn::intent_detail_grammar::{DetailSlot, GroundedDetailAssignment};
use crate::turn::intent_detail_syntax::{
    grounded_detail_assignment_scope_with_slot, grounded_static_detail_continuation,
};
use crate::turn::intent_interpretation::{CloseAuthorizationV2, IntentLocaleHintV2};
use crate::turn::intent_metalinguistic_scope::first_copy_carrier_index;

use super::UnquotedGroundingLink;

mod close;
mod locale;
mod syntax;

use close::*;
use locale::*;
use syntax::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) struct GroundedClosedAxes {
    pub(in crate::turn) locale: Result<IntentLocaleHintV2, ClosedAxisGroundingError>,
    pub(in crate::turn) close_authorization: Result<CloseAuthorizationV2, ClosedAxisGroundingError>,
}

impl Default for GroundedClosedAxes {
    fn default() -> Self {
        Self {
            locale: Ok(IntentLocaleHintV2::Unspecified),
            close_authorization: Ok(CloseAuthorizationV2::NotRequested),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) enum ClosedAxisGroundingError {
    AmbiguousLocale,
    ConflictingLocale,
    UnsupportedLocale,
    AmbiguousClose,
    ConflictingClose,
    UnsupportedClose,
}

pub(super) struct ClosedAxesAccumulator {
    locale: IntentLocaleHintV2,
    close_authorization: CloseAuthorizationV2,
    locale_error: Option<ClosedAxisGroundingError>,
    close_error: Option<ClosedAxisGroundingError>,
    previous_locale_branch: Option<IntentLocaleHintV2>,
    previous_locale_alternative: bool,
    previous_close_scope: bool,
    previous_close_branch: Option<CloseAuthorizationV2>,
    pending_correction: bool,
    pending_korean_locale_default: Option<PendingLocaleDefault>,
    detector_scope: bool,
    previous_managed_detail_slot: Option<DetailSlot>,
}

pub(super) struct ClosedAxisObservation<'a> {
    pub(super) active_value: &'a str,
    pub(super) source_value: &'a str,
    pub(super) literal_source_value: &'a str,
    pub(super) link: UnquotedGroundingLink,
    pub(super) continuation: Option<&'a str>,
    pub(super) continuation_source: Option<&'a str>,
    pub(super) continuation_link: Option<UnquotedGroundingLink>,
    pub(super) operative_consequent: bool,
}

#[derive(Clone, Copy)]
struct PendingLocaleDefault {
    locale: IntentLocaleHintV2,
    alternative: bool,
    correction: bool,
}

impl Default for ClosedAxesAccumulator {
    fn default() -> Self {
        Self {
            locale: IntentLocaleHintV2::Unspecified,
            close_authorization: CloseAuthorizationV2::NotRequested,
            locale_error: None,
            close_error: None,
            previous_locale_branch: None,
            previous_locale_alternative: false,
            previous_close_scope: false,
            previous_close_branch: None,
            pending_correction: false,
            pending_korean_locale_default: None,
            detector_scope: false,
            previous_managed_detail_slot: None,
        }
    }
}

impl ClosedAxesAccumulator {
    #[cfg(test)]
    pub(super) fn observe(
        &mut self,
        raw_value: &str,
        link: UnquotedGroundingLink,
        continuation: Option<&str>,
        alternative_continuation: Option<&str>,
    ) {
        self.observe_with_source(ClosedAxisObservation {
            active_value: raw_value,
            source_value: raw_value,
            literal_source_value: raw_value,
            link,
            continuation,
            continuation_source: continuation,
            continuation_link: continuation.map(|_| {
                if alternative_continuation.is_some() {
                    UnquotedGroundingLink::Alternative
                } else {
                    UnquotedGroundingLink::Additive
                }
            }),
            operative_consequent: false,
        });
    }

    pub(super) fn observe_with_source(&mut self, observation: ClosedAxisObservation<'_>) {
        let ClosedAxisObservation {
            active_value,
            source_value,
            literal_source_value,
            link,
            continuation,
            continuation_source,
            continuation_link,
            operative_consequent,
        } = observation;
        let alternative_continuation =
            continuation.filter(|_| continuation_link == Some(UnquotedGroundingLink::Alternative));
        let copy_carrier_index = first_copy_carrier_index(source_value);
        let value = active_value
            .get(..first_copy_carrier_index(active_value).unwrap_or(active_value.len()))
            .unwrap_or(active_value);
        if self.detector_scope
            && link == UnquotedGroundingLink::Additive
            && starts_closed_axis_imperative(value)
        {
            self.detector_scope = false;
        }
        if self.detector_scope || closed_axis_detector_context(value) {
            self.previous_locale_branch = None;
            self.previous_locale_alternative = false;
            self.previous_close_scope = false;
            self.previous_close_branch = None;
            self.pending_correction = false;
            self.pending_korean_locale_default = None;
            self.detector_scope = self.detector_scope || opens_closed_axis_detector_scope(value);
            self.previous_managed_detail_slot = None;
            return;
        }
        if opens_closed_axis_detector_scope(value) {
            self.break_ephemeral_scope();
            self.detector_scope = true;
            self.previous_managed_detail_slot = None;
            return;
        }
        let words = words(value);
        let directive_words = strip_directive_prefixes(&words);
        let linked_alternative = link == UnquotedGroundingLink::Alternative
            || (link == UnquotedGroundingLink::Detached && starts_alternative_prefix(value));
        let linked_additive = link == UnquotedGroundingLink::Additive;
        let followed_by_alternative = alternative_continuation.is_some();
        let linked_locale_alternative = linked_alternative
            || (self.previous_locale_alternative
                && matches!(
                    link,
                    UnquotedGroundingLink::Additive | UnquotedGroundingLink::Detached
                ));
        let inline_locale_alternative = inline_locale_alternative(value, &words);
        let locale_alternative = linked_locale_alternative || inline_locale_alternative;
        let inline_close_alternative = inline_close_alternative(value, directive_words);
        let close_alternative = linked_alternative || inline_close_alternative;
        let standalone_correction = standalone_correction(value);
        let correction =
            correction_directive(value) || self.pending_correction || standalone_correction;
        let locale_directive = locale_directive(directive_words, locale_alternative);
        let correction_locale = correction
            .then(|| locale_branch_value(directive_words))
            .flatten();
        let locale_directive = match (locale_directive, correction_locale) {
            (AxisDirective::None, Some(locale)) => AxisDirective::Value(locale),
            (directive, _) => directive,
        };
        let deferred_bare_locale =
            followed_by_alternative && bare_locale_directive(directive_words);
        let active_locale_directive = if deferred_bare_locale {
            AxisDirective::None
        } else {
            locale_directive
        };
        let continued_locale = if link == UnquotedGroundingLink::Additive
            && (korean_default_continuation(directive_words)
                || !matches!(active_locale_directive, AxisDirective::None))
        {
            self.pending_korean_locale_default.take()
        } else {
            self.pending_korean_locale_default = None;
            None
        };
        let locale_alternative = locale_alternative
            || continued_locale.is_some_and(|continuation| continuation.alternative);
        let locale_correction = !locale_alternative
            && (correction || continued_locale.is_some_and(|continuation| continuation.correction));
        let continued_locale = continued_locale.map(|continuation| continuation.locale);
        let combined_locale_directive =
            combine_locale_directive(active_locale_directive, continued_locale);
        let pending_locale_fragment =
            matches!(active_locale_directive, AxisDirective::None) && !deferred_bare_locale;
        let pending_locale_fragment = pending_locale_fragment
            .then(|| korean_locale_default_fragment(directive_words))
            .flatten();
        let locale_branch = match combined_locale_directive {
            AxisDirective::Value(locale) => Some(locale),
            AxisDirective::None => {
                pending_locale_fragment.or_else(|| locale_branch_value(directive_words))
            }
            AxisDirective::Conflict => None,
        };
        if inline_locale_alternative {
            self.record_locale_error(ClosedAxisGroundingError::AmbiguousLocale);
        }
        if linked_locale_alternative {
            match (
                self.previous_locale_branch,
                combined_locale_directive,
                locale_branch,
            ) {
                (None, AxisDirective::Value(_) | AxisDirective::Conflict, _)
                | (None, AxisDirective::None, Some(_)) => {
                    self.record_locale_error(ClosedAxisGroundingError::AmbiguousLocale);
                }
                (Some(left), AxisDirective::Value(right), _) if left != right => {
                    self.record_locale_error(ClosedAxisGroundingError::AmbiguousLocale);
                }
                (Some(left), AxisDirective::None, Some(right)) if left != right => {
                    self.record_locale_error(ClosedAxisGroundingError::AmbiguousLocale);
                }
                (Some(_), AxisDirective::None, None)
                    if unsupported_locale_alternative_branch(value, directive_words) =>
                {
                    self.record_locale_error(ClosedAxisGroundingError::AmbiguousLocale);
                }
                _ => {}
            }
        }
        if linked_additive
            && self
                .previous_locale_branch
                .zip(locale_branch)
                .is_some_and(|(left, right)| left != right)
        {
            self.record_locale_error(ClosedAxisGroundingError::ConflictingLocale);
        }
        if linked_additive
            && self.previous_locale_branch.is_some()
            && locale_branch.is_none()
            && unsupported_locale_alternative_branch(value, directive_words)
        {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
        let exhaustive_locale_scope = alternative_continuation
            .is_some_and(|continuation| exhaustive_locale_scope(value, continuation));
        let managed_detail_assignment = grounded_detail_assignment_scope_with_slot(source_value)
            .or_else(|| {
                (link == UnquotedGroundingLink::Additive)
                    .then_some(self.previous_managed_detail_slot)
                    .flatten()
                    .filter(|slot| grounded_static_detail_continuation(literal_source_value, *slot))
                    .map(|slot| (GroundedDetailAssignment::Static, slot))
            });
        self.previous_managed_detail_slot =
            managed_detail_assignment.and_then(|(assignment, slot)| {
                (assignment == GroundedDetailAssignment::Static).then_some(slot)
            });
        let copy_carrier_locale = locale_branch
            .or(self.previous_locale_branch)
            .or((self.locale != IntentLocaleHintV2::Unspecified).then_some(self.locale));
        if !exhaustive_locale_scope
            && (unsupported_locale_modifier(value, directive_words, locale_branch)
                || unsupported_managed_detail_locale_grounding(
                    managed_detail_assignment,
                    continuation_source.or(continuation),
                    continuation_link,
                    copy_carrier_locale,
                    operative_consequent,
                )
                || copy_carrier_index.is_some_and(|index| {
                    managed_detail_assignment.is_none()
                        && legacy_copy_carrier_locale_scope(source_value, index)
                        && (operative_consequent
                            || source_value.get(index..).is_some_and(|tail| {
                                unsupported_copy_carrier_locale_modifier(tail, copy_carrier_locale)
                            })
                            || continuation.is_some_and(|tail| {
                                unsupported_copy_carrier_locale_continuation(
                                    tail,
                                    continuation_link,
                                    copy_carrier_locale,
                                )
                            }))
                }))
        {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
        if !exhaustive_locale_scope
            && locale_branch.is_some()
            && alternative_continuation.is_some_and(unsupported_locale_condition_continuation)
        {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
        if self
            .previous_locale_branch
            .is_some_and(|previous| connected_locale_modifier(value, directive_words, previous))
        {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
        if !locale_alternative
            && !locale_correction
            && self
                .previous_locale_branch
                .zip(locale_branch)
                .is_some_and(|(left, right)| left != right)
            && locale_fragment_directive(directive_words)
        {
            self.record_locale_error(ClosedAxisGroundingError::ConflictingLocale);
        }
        if locale_correction && negated_locale_retraction(value, directive_words) {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
        self.observe_locale(
            combined_locale_directive,
            locale_alternative,
            locale_correction,
        );
        if let Some(locale) = pending_locale_fragment {
            self.pending_korean_locale_default = Some(PendingLocaleDefault {
                locale,
                alternative: locale_alternative,
                correction: locale_correction,
            });
        }
        if matches!(active_locale_directive, AxisDirective::None)
            && !deferred_bare_locale
            && continued_locale.is_none()
            && pending_locale_fragment.is_none()
            && (unsupported_locale_request(value, directive_words)
                || unsupported_accumulated_locale_request(value, directive_words, self.locale))
        {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
        self.previous_locale_branch = if standalone_correction && locale_branch.is_none() {
            self.previous_locale_branch
        } else if deferred_bare_locale {
            match locale_directive {
                AxisDirective::Value(locale) => Some(locale),
                AxisDirective::None | AxisDirective::Conflict => None,
            }
        } else {
            locale_branch
        };
        self.previous_locale_alternative = split_locale_alternative_prefix(value, directive_words)
            && self.previous_locale_branch.is_some();
        let current_close_scope = direct_close_scope(value, directive_words);
        let inherited_close_scope =
            (close_alternative || linked_additive || correction) && self.previous_close_scope;
        let close_branch = close_branch_hint(
            value,
            directive_words,
            inherited_close_scope,
            close_alternative,
        );
        let direct_close = close_directive(value, directive_words, inherited_close_scope);
        if continuation.is_some_and(unsupported_close_condition_continuation)
            && (!matches!(direct_close, AxisDirective::None)
                || !matches!(close_branch, AxisDirective::None))
        {
            self.record_close_error(ClosedAxisGroundingError::UnsupportedClose);
        }
        let active_close_alternative = close_alternative
            && (current_close_scope
                || self.previous_close_scope
                || !matches!(direct_close, AxisDirective::None));
        let active_close_additive = linked_additive
            && (current_close_scope
                || self.previous_close_scope
                || !matches!(direct_close, AxisDirective::None)
                || !matches!(close_branch, AxisDirective::None));
        let active_close_correction = correction
            && self.previous_close_branch.is_some()
            && (!matches!(direct_close, AxisDirective::None)
                || !matches!(close_branch, AxisDirective::None));
        if inline_close_alternative {
            self.record_close_error(ClosedAxisGroundingError::AmbiguousClose);
        }
        if unsupported_close_modifier(value, directive_words)
            || (linked_additive
                && self.previous_close_scope
                && matches!(close_branch, AxisDirective::None)
                && unsupported_connected_close_modifier(value, directive_words))
        {
            self.record_close_error(ClosedAxisGroundingError::UnsupportedClose);
        }
        if self.previous_close_branch.is_some()
            && connected_close_restriction(value, directive_words, continuation)
        {
            self.record_close_error(ClosedAxisGroundingError::UnsupportedClose);
        }
        if linked_alternative && active_close_alternative {
            let current_close_branch = match (direct_close, close_branch) {
                (AxisDirective::Conflict, _) | (_, AxisDirective::Conflict) => {
                    AxisDirective::Conflict
                }
                (AxisDirective::Value(left), AxisDirective::Value(right)) if left != right => {
                    AxisDirective::Conflict
                }
                (AxisDirective::Value(value), _) | (_, AxisDirective::Value(value)) => {
                    AxisDirective::Value(value)
                }
                (AxisDirective::None, AxisDirective::None) => AxisDirective::None,
            };
            match (self.previous_close_branch, current_close_branch) {
                (None, AxisDirective::Value(_) | AxisDirective::Conflict) => {
                    self.record_close_error(ClosedAxisGroundingError::AmbiguousClose);
                }
                (Some(_), AxisDirective::None)
                    if unsupported_close_alternative_branch(value, directive_words) =>
                {
                    self.record_close_error(ClosedAxisGroundingError::AmbiguousClose);
                }
                _ => {}
            }
        }
        let close_directive = if active_close_correction {
            merge_alternative_close_branch(direct_close, None, close_branch, true)
        } else {
            merge_alternative_close_branch(
                direct_close,
                self.previous_close_branch,
                close_branch,
                active_close_alternative || active_close_additive,
            )
        };
        self.observe_close(
            close_directive,
            active_close_alternative,
            active_close_correction || (!active_close_alternative && correction),
        );
        if matches!(close_directive, AxisDirective::None)
            && unsupported_close_request(value, directive_words)
        {
            self.record_close_error(ClosedAxisGroundingError::UnsupportedClose);
        }
        if !(standalone_correction
            && matches!(close_branch, AxisDirective::None)
            && matches!(close_directive, AxisDirective::None))
        {
            self.previous_close_scope = current_close_scope;
            self.previous_close_branch = match close_branch {
                AxisDirective::Value(value) => Some(value),
                AxisDirective::None => match close_directive {
                    AxisDirective::Value(value) => Some(value),
                    AxisDirective::None | AxisDirective::Conflict => None,
                },
                AxisDirective::Conflict => None,
            };
        }
        self.pending_correction = standalone_correction;
    }

    pub(super) fn observe_copy_scope_continuation(
        &mut self,
        source_value: &str,
        operative_consequent: bool,
    ) {
        let source_words = words(source_value);
        let selected = (self.locale != IntentLocaleHintV2::Unspecified).then_some(self.locale);
        let managed_detail_assignment = grounded_detail_assignment_scope_with_slot(source_value);
        if unsupported_accumulated_locale_request(source_value, &source_words, self.locale)
            || unsupported_managed_detail_locale_grounding(
                managed_detail_assignment,
                None,
                None,
                selected,
                operative_consequent,
            )
        {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
    }

    pub(super) fn finish(mut self) -> GroundedClosedAxes {
        if self.pending_korean_locale_default.is_some() {
            self.record_locale_error(ClosedAxisGroundingError::UnsupportedLocale);
        }
        GroundedClosedAxes {
            locale: self.locale_error.map_or(Ok(self.locale), Err),
            close_authorization: self.close_error.map_or(Ok(self.close_authorization), Err),
        }
    }

    pub(super) fn break_ephemeral_scope(&mut self) {
        self.previous_locale_branch = None;
        self.previous_locale_alternative = false;
        self.previous_close_scope = false;
        self.previous_close_branch = None;
        self.pending_correction = false;
        self.pending_korean_locale_default = None;
        self.detector_scope = false;
        self.previous_managed_detail_slot = None;
    }

    pub(super) fn end_semantic_sentence(&mut self) {
        self.detector_scope = false;
        self.previous_locale_alternative = false;
        self.previous_managed_detail_slot = None;
    }

    fn observe_locale(
        &mut self,
        directive: AxisDirective<IntentLocaleHintV2>,
        alternative: bool,
        correction: bool,
    ) {
        match directive {
            AxisDirective::None => {}
            AxisDirective::Value(locale) => {
                if correction {
                    self.locale = locale;
                    self.locale_error = None;
                } else if alternative
                    && self.locale != IntentLocaleHintV2::Unspecified
                    && self.locale != locale
                {
                    self.record_locale_error(ClosedAxisGroundingError::AmbiguousLocale);
                } else if self.locale == IntentLocaleHintV2::Unspecified || self.locale == locale {
                    self.locale = locale;
                } else {
                    self.record_locale_error(ClosedAxisGroundingError::ConflictingLocale);
                }
            }
            AxisDirective::Conflict => {
                self.record_locale_error(if alternative {
                    ClosedAxisGroundingError::AmbiguousLocale
                } else {
                    ClosedAxisGroundingError::ConflictingLocale
                });
            }
        }
    }

    fn observe_close(
        &mut self,
        directive: AxisDirective<CloseAuthorizationV2>,
        alternative: bool,
        correction: bool,
    ) {
        match directive {
            AxisDirective::None => {}
            AxisDirective::Value(close_authorization) => {
                if correction {
                    self.close_authorization = close_authorization;
                    self.close_error = None;
                } else if alternative
                    && self.close_authorization != CloseAuthorizationV2::NotRequested
                    && self.close_authorization != close_authorization
                {
                    self.record_close_error(ClosedAxisGroundingError::AmbiguousClose);
                } else if self.close_authorization == CloseAuthorizationV2::NotRequested
                    || self.close_authorization == close_authorization
                {
                    self.close_authorization = close_authorization;
                } else {
                    self.record_close_error(ClosedAxisGroundingError::ConflictingClose);
                }
            }
            AxisDirective::Conflict => {
                self.record_close_error(if alternative {
                    ClosedAxisGroundingError::AmbiguousClose
                } else {
                    ClosedAxisGroundingError::ConflictingClose
                });
            }
        }
    }

    fn record_locale_error(&mut self, error: ClosedAxisGroundingError) {
        if self.locale_error == Some(ClosedAxisGroundingError::UnsupportedLocale)
            && error == ClosedAxisGroundingError::ConflictingLocale
        {
            self.locale_error = Some(error);
            return;
        }
        self.locale_error.get_or_insert(error);
    }

    fn record_close_error(&mut self, error: ClosedAxisGroundingError) {
        self.close_error.get_or_insert(error);
    }
}

#[derive(Clone, Copy)]
enum AxisDirective<T> {
    None,
    Value(T),
    Conflict,
}

pub(in crate::turn) fn closed_axis_semantic_authority(value: &str) -> bool {
    let value = value.to_lowercase();
    let value = value
        .get(..first_copy_carrier_index(&value).unwrap_or(value.len()))
        .unwrap_or(&value);
    if closed_axis_detector_context(value) || opens_closed_axis_detector_scope(value) {
        return false;
    }
    let words = words(value);
    let directive_words = strip_directive_prefixes(&words);
    !matches!(
        locale_directive(directive_words, false),
        AxisDirective::None
    ) || unsupported_locale_request(value, directive_words)
        || !matches!(close_directive(value, &words, false), AxisDirective::None)
        || unsupported_close_request(value, &words)
}

pub(in crate::turn) fn grounded_closed_axis_restatement(
    value: &str,
) -> (Option<IntentLocaleHintV2>, Option<CloseAuthorizationV2>) {
    let value = value.to_lowercase();
    let value = value
        .get(..first_copy_carrier_index(&value).unwrap_or(value.len()))
        .unwrap_or(&value);
    let words = words(value);
    let directive_words = strip_directive_prefixes(&words);
    let locale = match locale_directive(directive_words, false) {
        AxisDirective::Value(locale) => Some(locale),
        AxisDirective::None | AxisDirective::Conflict => None,
    };
    let close_authorization = match close_directive(value, &words, false) {
        AxisDirective::Value(close_authorization) => Some(close_authorization),
        AxisDirective::None | AxisDirective::Conflict => None,
    };
    (locale, close_authorization)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_carrier_locale_scope_ignores_independent_runtime_continuations() {
        let controls = super::super::grounded_request_controls(
            "Build a managed private study-room automation. Use English defaults except that the Help button label is exactly 'Guide' and when clicked post the help panel.",
        );
        assert_eq!(controls.closed_axes.locale, Ok(IntentLocaleHintV2::En));
    }

    #[test]
    fn copy_carriers_without_a_locale_never_synthesize_locale_authority() {
        let controls = super::super::grounded_request_controls(
            "Build a managed private study-room automation where the Help button label is 'Guide' and create a separate summary panel.",
        );
        assert_eq!(
            controls.closed_axes.locale,
            Ok(IntentLocaleHintV2::Unspecified)
        );
    }

    #[test]
    fn copy_carrier_locale_scope_rejects_late_scoped_foreign_defaults() {
        let controls = super::super::grounded_request_controls(
            "Build a managed private study-room automation. Use English defaults except that the Help button label is exactly 'Guide' and its response is exactly 'Read' and on weekends use French defaults.",
        );
        assert_eq!(
            controls.closed_axes.locale,
            Err(ClosedAxisGroundingError::UnsupportedLocale)
        );
    }

    #[test]
    fn locale_scope_uses_the_complete_static_detail_assignment_grammar() {
        for human in [
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help button label is set to 'Guide'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help button label as 'Guide'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help button label named 'Guide'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help button label is Guide.",
            "Build a managed private study-room automation. Use English defaults except that the Help button label is Mobile.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the channel name uses prefix 'study-'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the modal title is 'Room' plus the Help button label is 'Guide'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help button label is 'Guide' plus the modal title is 'Room'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help button label is 'Guide' and the modal title is 'Room'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help button label is 'Guide' and the channel name uses prefix 'study-'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the modal title is 'Room', and leave room closing disabled.",
            "Build a managed private study-room automation. Use English defaults except that the Help button label is 'Guide' and leave room closing disabled.",
            "Build a managed private study-room automation. Use English defaults except that Help button label is 'Guide' and leave room closing disabled.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: 도움말 버튼 라벨을 「안내」로 변경해.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: 도움말 버튼 라벨을 「안내」으로 변경해.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: 도움말 버튼 라벨을 안내로 변경해.",
            "Build a managed private study-room automation. Use English defaults. Archive the Help button label in an audit log.",
            "Build a managed private study-room automation. Use English defaults. For clarity, set the Help button label to 'Guide'.",
        ] {
            let controls = super::super::grounded_request_controls(human);
            assert_eq!(
                controls.closed_axes.locale,
                Ok(IntentLocaleHintV2::En),
                "static detail assignment was rejected for {human}"
            );
        }
    }

    #[test]
    fn locale_scope_preserves_literal_affix_continuations() {
        let controls = super::super::grounded_request_controls(
            "Build a managed private study-room automation. Use English defaults except for generated names: the channel name has prefix 'focus-' and suffix '-room', and the member-role name has prefix 'team-' and suffix '-members'.",
        );

        assert_eq!(controls.closed_axes.locale, Ok(IntentLocaleHintV2::En));
    }

    #[test]
    fn locale_scope_rejects_ungrounded_affix_continuations() {
        for human in [
            "Build a managed private study-room automation. Use English defaults except for generated names: the channel name has prefix 'focus-' and suffix.",
            "Build a managed private study-room automation. Use English defaults except for generated names: the channel name has prefix 'focus-' or suffix '-room'.",
            "Build a managed private study-room automation. Use English defaults except for generated names: the channel name has prefix 'focus-' and suffix '-room' when archived.",
        ] {
            let controls = super::super::grounded_request_controls(human);
            assert_eq!(
                controls.closed_axes.locale,
                Err(ClosedAxisGroundingError::UnsupportedLocale),
                "ungrounded affix continuation was accepted for {human}"
            );
        }
    }

    #[test]
    fn locale_scope_rejects_conditional_details_across_managed_surfaces() {
        for human in [
            "Build a managed private study-room automation. Use English defaults. When the room is archived, change the Help button label to 'Guide'.",
            "Build a managed private study-room automation. Use English defaults. On weekends set the Help button label to 'Guide'.",
            "Build a managed private study-room automation. Use English defaults. On weekends, set the Help button label to 'Guide'.",
            "Build a managed private study-room automation. Use English defaults. After a restart, set the modal title to 'Room'.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the modal title changes when archived.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the launcher content changes on weekends.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the room name label changes after a restart.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the Help response changes when archived.",
            "Build a managed private study-room automation. Use English defaults except for these exact overrides: the channel name uses prefix 'study-' when archived.",
        ] {
            let controls = super::super::grounded_request_controls(human);
            assert_eq!(
                controls.closed_axes.locale,
                Err(ClosedAxisGroundingError::UnsupportedLocale),
                "conditional detail was accepted for {human}"
            );
        }
    }

    #[test]
    fn operative_detail_consequents_reach_the_closed_axis_guard() {
        let human = "Build a managed private study-room automation. Use English defaults. When the room is archived, change the Help button label to 'Guide'.";
        let grounding = crate::turn::intent_boundary_grounding::unquoted_grounding_text(human)
            .expect("grounding text");
        let detail = grounding
            .sentences
            .iter()
            .flatten()
            .find(|unit| unit.text.contains("help button label"))
            .expect("detail consequent");
        assert_eq!(detail.operative_authority, Some(true));
        assert_eq!(
            crate::turn::intent_detail_syntax::grounded_detail_assignment_scope(&detail.text),
            Some(crate::turn::intent_detail_grammar::GroundedDetailAssignment::Static)
        );
        let controls = super::super::grounded_request_controls(human);
        assert_eq!(
            controls.closed_axes.locale,
            Err(ClosedAxisGroundingError::UnsupportedLocale)
        );
    }

    #[test]
    fn copy_scope_continuations_preserve_late_locale_evidence() {
        let controls = super::super::grounded_request_controls(
            "Build a managed private study-room automation. Use English defaults except that the Help button says 'Guide' and its response is 'Read' and on weekends use French defaults.",
        );
        assert_eq!(
            controls.closed_axes.locale,
            Err(ClosedAxisGroundingError::UnsupportedLocale)
        );
    }

    #[test]
    fn copy_carrier_locale_scope_rejects_attached_dynamic_continuations() {
        for human in [
            "Build a managed private study-room automation. Use English defaults except that the room Help button label is exactly 'Guide' and changes on weekends.",
            "Build a managed private study-room automation. Use English defaults except that the room Help button label is exactly 'Guide' when archived.",
            "Build a managed private study-room automation. Use English defaults except that the room Help button label is exactly 'Guide' and set its response to 'Read' on weekends.",
        ] {
            let controls = super::super::grounded_request_controls(human);
            assert_eq!(
                controls.closed_axes.locale,
                Err(ClosedAxisGroundingError::UnsupportedLocale),
                "dynamic copy continuation was accepted for {human}"
            );
        }
    }

    #[test]
    fn accumulated_locale_scope_allows_explicit_foreign_default_exclusion() {
        let controls = super::super::grounded_request_controls(
            "Build a managed private study-room automation. Use English defaults. Do not use French defaults.",
        );
        assert_eq!(controls.closed_axes.locale, Ok(IntentLocaleHintV2::En));
    }

    #[test]
    fn accumulated_locale_scope_rejects_selected_default_retraction() {
        let controls = super::super::grounded_request_controls(
            "Build a managed private study-room automation. Use English defaults. Do not use English defaults.",
        );
        assert_eq!(
            controls.closed_axes.locale,
            Err(ClosedAxisGroundingError::UnsupportedLocale)
        );
    }

    fn measured_work(repetitions: usize) -> usize {
        reset_closed_axis_work();
        let mut accumulator = ClosedAxesAccumulator::default();
        for _ in 0..repetitions {
            accumulator.observe(
                "keep the ordinary panel response unchanged",
                UnquotedGroundingLink::Detached,
                None,
                None,
            );
        }
        let _ = accumulator.finish();
        closed_axis_work()
    }

    fn measured_input_work(repetitions: usize) -> usize {
        let value = format!("use korean defaults {}", "on desktop ".repeat(repetitions));
        reset_closed_axis_work();
        let mut accumulator = ClosedAxesAccumulator::default();
        accumulator.observe(&value, UnquotedGroundingLink::Detached, None, None);
        let _ = accumulator.finish();
        closed_axis_work()
    }

    #[test]
    fn closed_axis_grounding_work_scales_linearly() {
        let small = measured_work(128);
        let large = measured_work(256);
        assert!(small > 0);
        assert_eq!(large, small.saturating_mul(2));

        let small_input = measured_input_work(128);
        let large_input = measured_input_work(256);
        assert!(small_input > 0);
        assert!(large_input <= small_input.saturating_mul(3));
    }

    #[test]
    fn split_locale_alternatives_fail_closed() {
        let mut accumulator = ClosedAxesAccumulator::default();
        accumulator.observe(
            "use english",
            UnquotedGroundingLink::Detached,
            Some("한국어 기본 문구하고 이름을 사용해"),
            Some("한국어 기본 문구하고 이름을 사용해"),
        );
        assert_eq!(
            accumulator.previous_locale_branch,
            Some(IntentLocaleHintV2::En)
        );
        accumulator.observe(
            "한국어 기본 문구하고 이름을 사용해",
            UnquotedGroundingLink::Alternative,
            None,
            None,
        );
        assert_eq!(
            accumulator.finish().locale,
            Err(ClosedAxisGroundingError::AmbiguousLocale)
        );
    }

    #[test]
    fn natural_close_permission_restatements_are_closed() {
        for (value, expected) in [
            (
                "only the room creator may use the close button",
                CloseAuthorizationV2::CreatorOnly,
            ),
            (
                "allow all members to use the close button",
                CloseAuthorizationV2::AnyMember,
            ),
            (
                "keep room closing turned off",
                CloseAuthorizationV2::Disabled,
            ),
            ("leave closing turned off", CloseAuthorizationV2::Disabled),
        ] {
            assert_eq!(grounded_closed_axis_restatement(value).1, Some(expected));
        }
    }
}
