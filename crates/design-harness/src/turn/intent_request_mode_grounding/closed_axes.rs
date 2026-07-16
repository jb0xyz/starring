use crate::turn::intent_interpretation::{CloseAuthorizationV2, IntentLocaleHintV2};
use crate::turn::intent_metalinguistic_scope::first_copy_carrier_index;

use super::patterns::KOREAN_TARGET_PARTICLES;
use super::UnquotedGroundingLink;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static CLOSED_AXIS_WORK: Cell<usize> = const { Cell::new(0) };
}

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
        }
    }
}

impl ClosedAxesAccumulator {
    pub(super) fn observe(
        &mut self,
        raw_value: &str,
        link: UnquotedGroundingLink,
        continuation: Option<&str>,
        alternative_continuation: Option<&str>,
    ) {
        let value = raw_value
            .get(..first_copy_carrier_index(raw_value).unwrap_or(raw_value.len()))
            .unwrap_or(raw_value);
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
            return;
        }
        if opens_closed_axis_detector_scope(value) {
            self.break_ephemeral_scope();
            self.detector_scope = true;
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
        if !exhaustive_locale_scope
            && unsupported_locale_modifier(value, directive_words, locale_branch)
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
            && unsupported_locale_request(value, directive_words)
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
    }

    pub(super) fn end_semantic_sentence(&mut self) {
        self.detector_scope = false;
        self.previous_locale_alternative = false;
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

fn combine_locale_directive(
    directive: AxisDirective<IntentLocaleHintV2>,
    continued_locale: Option<IntentLocaleHintV2>,
) -> AxisDirective<IntentLocaleHintV2> {
    match (directive, continued_locale) {
        (AxisDirective::None, Some(locale)) => AxisDirective::Value(locale),
        (AxisDirective::Value(current), Some(continued)) if current != continued => {
            AxisDirective::Conflict
        }
        (directive, _) => directive,
    }
}

fn bare_locale_directive(words: &[&str]) -> bool {
    matches!(words, ["use", "english" | "korean"])
}

fn locale_branch_value(words: &[&str]) -> Option<IntentLocaleHintV2> {
    let words = match words {
        ["choose", "between", rest @ ..] | ["use", "either", rest @ ..] => rest,
        ["use", rest @ ..] => rest,
        _ => words,
    };
    let candidate = words.first().copied()?;
    locale_token_value(candidate)
}

fn locale_token_value(candidate: &str) -> Option<IntentLocaleHintV2> {
    let candidate = candidate
        .strip_suffix('로')
        .or_else(|| candidate.strip_suffix('를'))
        .unwrap_or(candidate);
    match candidate {
        "english" | "영어" => Some(IntentLocaleHintV2::En),
        "korean" | "한국어" => Some(IntentLocaleHintV2::Ko),
        _ => None,
    }
}

fn locale_directive(words: &[&str], alternative_branch: bool) -> AxisDirective<IntentLocaleHintV2> {
    if contains_sequence(words, &["use", "korean", "rather", "than", "english"]) {
        return AxisDirective::Value(IntentLocaleHintV2::Ko);
    }
    if contains_sequence(words, &["use", "english", "rather", "than", "korean"]) {
        return AxisDirective::Value(IntentLocaleHintV2::En);
    }
    if let Some(locale) = korean_locale_correction(words) {
        return AxisDirective::Value(locale);
    }
    let english = english_locale_directive(words, "english", alternative_branch)
        || korean_language_directive(words, "영어");
    let korean = english_locale_directive(words, "korean", alternative_branch)
        || korean_language_directive(words, "한국어");
    match (english, korean) {
        (true, true) => AxisDirective::Conflict,
        (true, false) => AxisDirective::Value(IntentLocaleHintV2::En),
        (false, true) => AxisDirective::Value(IntentLocaleHintV2::Ko),
        (false, false) => AxisDirective::None,
    }
}

fn english_locale_directive(words: &[&str], locale: &str, alternative_branch: bool) -> bool {
    let direct_use = words.starts_with(&["use", locale])
        && words.get(2).is_none_or(|word| {
            matches!(
                *word,
                "copy"
                    | "default"
                    | "defaults"
                    | "interface"
                    | "label"
                    | "labels"
                    | "language"
                    | "naming"
                    | "responses"
                    | "ui"
            )
        })
        || (words.starts_with(&["use", locale, "for"])
            && words.iter().skip(3).any(|word| locale_surface_word(word)));
    let language_setting = words.starts_with(&["set", "language", "to", locale])
        || words.starts_with(&["set", "the", "language", "to", locale])
        || words.starts_with(&["set", "locale", "to", locale])
        || words.starts_with(&["set", "the", "locale", "to", locale])
        || words.starts_with(&["set", "response", "language", "to", locale])
        || words.starts_with(&["set", "the", "response", "language", "to", locale])
        || words.starts_with(&["set", "interface", "to", locale])
        || words.starts_with(&["set", "the", "interface", "to", locale]);
    let direct_response = matches!(
        words,
        ["answer" | "reply" | "respond", "in" | "using", selected, ..] if *selected == locale
    ) || (words.starts_with(&["write"])
        && (contains_sequence(words, &["response", "in", locale])
            || contains_sequence(words, &["responses", "in", locale])
            || contains_sequence(words, &["copy", "in", locale])));
    let declarative_response = matches!(
        words,
        ["the", "response" | "responses", "should" | "must", "be", "in", selected, ..]
            if *selected == locale
    );
    let inherited_default = alternative_branch
        && matches!(words, [selected, "default" | "defaults", ..] if *selected == locale);
    let interface_selection = words.starts_with(&["use", locale, "for", "the", "interface"])
        || words.starts_with(&["use", locale, "for", "interface"])
        || words.starts_with(&["all", "ui", "copy", "should", "be", locale])
        || words.starts_with(&["all", "interface", "copy", "should", "be", locale])
        || words.starts_with(&["all", "labels", "should", "be", locale])
        || words.starts_with(&["the", "interface", "language", "must", "be", locale])
        || words.starts_with(&["the", "interface", "language", "should", "be", locale])
        || matches!(words, ["use", selected, "throughout", ..] if *selected == locale);
    direct_use
        || language_setting
        || direct_response
        || declarative_response
        || interface_selection
        || inherited_default
}

fn korean_language_directive(words: &[&str], language: &str) -> bool {
    if korean_negative_directive(words) || has_korean_semantic_analysis(words) {
        return false;
    }
    let language_indexes = words
        .iter()
        .enumerate()
        .filter_map(|(index, word)| word.starts_with(language).then_some(index))
        .collect::<Vec<_>>();
    let first_language = language_indexes.first() == Some(&0);
    let setting = language_indexes
        .iter()
        .any(|index| korean_language_selection_at(words, *index));
    let default_output = first_language
        && korean_locale_output_surface(words)
        && words
            .iter()
            .any(|word| ["사용", "작성"].iter().any(|marker| word.contains(marker)));
    let direct_response = first_language
        && words
            .iter()
            .any(|word| word.contains("답변") || word.contains("응답"))
        && words
            .iter()
            .any(|word| word.contains("해") || word.contains("작성"));
    setting || default_output || direct_response
}

fn korean_locale_correction(words: &[&str]) -> Option<IntentLocaleHintV2> {
    if contains_sequence(words, &["영어로", "하지", "말고", "한국어로"]) {
        return Some(IntentLocaleHintV2::Ko);
    }
    if contains_sequence(words, &["한국어로", "하지", "말고", "영어로"]) {
        return Some(IntentLocaleHintV2::En);
    }
    for (index, window) in words.windows(3).enumerate() {
        let excluded = if window[0].starts_with("한국어") {
            IntentLocaleHintV2::Ko
        } else if window[0].starts_with("영어") {
            IntentLocaleHintV2::En
        } else {
            continue;
        };
        if !matches!(window[1], "말고" | "대신") {
            continue;
        }
        let selected = if window[2].starts_with("한국어") {
            IntentLocaleHintV2::Ko
        } else if window[2].starts_with("영어") {
            IntentLocaleHintV2::En
        } else {
            continue;
        };
        if selected != excluded && korean_language_selection_at(words, index.saturating_add(2)) {
            return Some(selected);
        }
    }
    None
}

fn korean_language_selection_at(words: &[&str], index: usize) -> bool {
    let Some(language) = words.get(index) else {
        return false;
    };
    let tail = words.get(index.saturating_add(1)..).unwrap_or_default();
    let output_surface = korean_locale_output_surface(words);
    let action = words.iter().any(|word| {
        word.contains("답변")
            || word.contains("설정")
            || word.contains("응답")
            || word.contains("작성")
            || word.contains("써")
            || matches!(*word, "해" | "해줘" | "해주세요")
    });
    let direct_use = tail
        .first()
        .is_some_and(|word| word.contains("사용") || word.contains("설정"));
    (language.ends_with('로') && (output_surface || action))
        || (language.ends_with('를') && direct_use)
        || (output_surface
            && words
                .iter()
                .any(|word| word.contains("사용") || word.contains("작성") || action))
}

fn korean_locale_output_surface(words: &[&str]) -> bool {
    words.iter().any(|word| {
        [
            "ui",
            "기본",
            "답변",
            "로케일",
            "문구",
            "이름",
            "언어",
            "응답",
        ]
        .iter()
        .any(|marker| word.to_lowercase().contains(marker))
    })
}

fn unsupported_locale_request(value: &str, words: &[&str]) -> bool {
    if korean_negative_directive(words) || has_korean_semantic_analysis(words) {
        return false;
    }
    let candidates = locale_candidate_tokens(words);
    let mentions_locale = !candidates.is_empty()
        || has_any(words, &["english", "korean"])
        || value.contains("한국어")
        || value.contains("영어");
    let direct_selection = words.first().is_some_and(|word| {
        matches!(
            *word,
            "answer" | "make" | "reply" | "respond" | "set" | "use" | "write"
        )
    }) || locale_fragment_candidate(words).is_some()
        || words.iter().any(|word| {
            [
                "기본", "답변", "문구", "사용", "설정", "응답", "작성", "해줘",
            ]
            .iter()
            .any(|marker| word.contains(marker))
        });
    let output_surface = has_any(
        words,
        &[
            "copy",
            "default",
            "defaults",
            "interface",
            "label",
            "labels",
            "language",
            "locale",
            "response",
            "responses",
        ],
    ) || words.iter().any(|word| {
        ["기본", "답변", "로케일", "문구", "언어", "응답"]
            .iter()
            .any(|marker| word.contains(marker))
    });
    mentions_locale
        && direct_selection
        && output_surface
        && (candidates.is_empty()
            || candidates
                .iter()
                .any(|candidate| !supported_locale_token(candidate)))
}

fn unsupported_locale_alternative_branch(_value: &str, words: &[&str]) -> bool {
    let candidates = locale_candidate_tokens(words);
    candidates
        .iter()
        .any(|candidate| !supported_locale_token(candidate))
        || words.first().is_some_and(|candidate| {
            locale_language_token(candidate)
                && !supported_locale_token(candidate)
                && words.get(1).is_some_and(|word| locale_surface_word(word))
        })
}

fn unsupported_locale_modifier(
    value: &str,
    words: &[&str],
    selected: Option<IntentLocaleHintV2>,
) -> bool {
    let Some(selected) = selected else {
        return false;
    };
    let has_distinct_locale = words
        .iter()
        .filter_map(|word| locale_token_value(word))
        .any(|locale| locale != selected);
    let recipe_detail_override = value.contains("except for these exact overrides")
        || value.contains("except for the following exact overrides")
        || value.contains("except for generated names")
        || value.contains("except that the ")
            && has_any(
                words,
                &[
                    "button", "channel", "content", "copy", "label", "name", "prefix", "response",
                    "suffix",
                ],
            );
    let exception = has_any(words, &["except", "excluding"]) || value.contains("제외");
    let conditional = has_any(
        words,
        &[
            "after", "before", "during", "if", "unless", "until", "when", "whenever", "while",
        ],
    );
    let restrictive = conditional || exception && !recipe_detail_override;
    let scoped_default = words
        .iter()
        .position(|word| matches!(*word, "default" | "defaults"))
        .is_some_and(|index| {
            words
                .get(index.saturating_add(1))
                .is_some_and(|word| matches!(*word, "on" | "during"))
        });
    let scoped_target = words.windows(2).any(|window| {
        matches!(window[0], "for" | "on")
            && matches!(
                window[1],
                "android"
                    | "desktop"
                    | "guests"
                    | "ios"
                    | "mobile"
                    | "weekdays"
                    | "weekends"
                    | "web"
            )
    });
    restrictive
        || scoped_default
        || scoped_target
        || has_distinct_locale && has_any(words, &["but"])
}

fn connected_locale_modifier(value: &str, words: &[&str], previous: IntentLocaleHintV2) -> bool {
    let restrictive = has_any(words, &["but", "except", "excluding", "unless", "while"])
        || value.contains("제외");
    if !restrictive || !words.iter().any(|word| locale_surface_word(word)) {
        return false;
    }
    words.iter().any(|word| {
        locale_token_value(word).is_some_and(|locale| locale != previous)
            || plausible_locale_token(word) && !supported_locale_token(word)
    })
}

fn exhaustive_locale_scope(value: &str, continuation: &str) -> bool {
    let continuation_words = words(continuation);
    value.contains("on desktop") && has_any(&continuation_words, &["mobile"])
        || value.contains("on mobile") && has_any(&continuation_words, &["desktop"])
}

fn unsupported_locale_condition_continuation(value: &str) -> bool {
    let words = words(value);
    !words.iter().any(|word| locale_language_token(word))
        && has_any(
            &words,
            &[
                "active",
                "approval",
                "archived",
                "desktop",
                "locked",
                "maintenance",
                "mobile",
                "scheduled",
                "weekdays",
                "weekends",
            ],
        )
}

fn locale_fragment_directive(words: &[&str]) -> bool {
    locale_fragment_candidate(words).is_some_and(supported_locale_token)
}

fn locale_fragment_candidate<'a>(words: &'a [&'a str]) -> Option<&'a str> {
    let candidate = words
        .first()
        .copied()
        .filter(|word| plausible_locale_token(word))?;
    if !words.get(1).is_some_and(|word| locale_surface_word(word))
        || has_any(
            words,
            &[
                "are",
                "called",
                "describe",
                "describes",
                "is",
                "means",
                "were",
            ],
        )
    {
        return None;
    }
    Some(candidate)
}

fn negated_locale_retraction(value: &str, words: &[&str]) -> bool {
    let mentions_supported = words.iter().any(|word| supported_locale_token(word));
    mentions_supported
        && (contains_sequence(words, &["do", "not", "use"])
            || matches!(words, ["don't" | "don’t" | "dont", "use", ..])
            || korean_negative_directive(words)
            || value.contains("사용하지 마")
            || value.contains("사용하지마"))
}

fn split_locale_alternative_prefix(value: &str, words: &[&str]) -> bool {
    value.contains("between ")
        || words.starts_with(&["choose", "between"])
        || words.starts_with(&["use", "either"])
}

fn locale_candidate_tokens<'a>(words: &'a [&'a str]) -> Vec<&'a str> {
    let mut candidates = Vec::new();
    if words.first() == Some(&"use") {
        let has_surface = words.iter().skip(2).any(|word| locale_surface_word(word));
        let candidate_index = usize::from(words.get(1) == Some(&"either")).saturating_add(1);
        if let Some(candidate) = words.get(candidate_index).filter(|candidate| {
            plausible_locale_token(candidate)
                && (has_surface
                    || words.len() == candidate_index.saturating_add(1)
                        && supported_locale_token(candidate))
        }) {
            candidates.push(*candidate);
        }
        if words.len() > candidate_index.saturating_add(1) {
            for candidate in words
                .iter()
                .skip(candidate_index.saturating_add(1))
                .take(2)
                .filter(|candidate| plausible_locale_token(candidate))
            {
                push_locale_candidate(&mut candidates, candidate);
            }
        }
    }
    let setting_candidate = match words {
        ["set", "language" | "locale", "to", candidate, ..]
        | ["set", "the", "language" | "locale", "to", candidate, ..]
        | ["answer" | "reply" | "respond", "in", candidate, ..] => Some(*candidate),
        ["set", "response", "language", "to", candidate, ..]
        | ["set", "the", "response", "language", "to", candidate, ..]
        | ["set", "interface", "to", candidate, ..]
        | ["set", "the", "interface", "to", candidate, ..]
        | ["write", "response" | "responses", "in", candidate, ..]
        | ["the", "response" | "responses", "should" | "must", "be", "in", candidate, ..] => {
            Some(*candidate)
        }
        ["all", "ui" | "interface", "copy", "should" | "must", "be", candidate, ..]
        | ["the", "interface", "language", "should" | "must", "be", candidate, ..] => {
            Some(*candidate)
        }
        _ => None,
    };
    if let Some(candidate) = setting_candidate {
        push_locale_candidate(&mut candidates, candidate);
    }
    for (index, word) in words.iter().enumerate() {
        if alternative_word(word) {
            if let Some(candidate) = words
                .get(index.saturating_add(1))
                .filter(|candidate| plausible_locale_token(candidate))
            {
                push_locale_candidate(&mut candidates, candidate);
            }
        }
    }
    if korean_locale_output_surface(words)
        && words.iter().any(|word| {
            word.contains("사용")
                || word.contains("설정")
                || word.contains("작성")
                || matches!(*word, "해" | "해줘" | "해주세요")
        })
    {
        for candidate in words
            .iter()
            .copied()
            .filter(|word| locale_language_token(word))
        {
            push_locale_candidate(&mut candidates, candidate);
        }
    }
    if let Some(candidate) = locale_fragment_candidate(words) {
        push_locale_candidate(&mut candidates, candidate);
    }
    candidates
}

fn push_locale_candidate<'a>(candidates: &mut Vec<&'a str>, candidate: &'a str) {
    if candidates.len() < 3 && !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn locale_surface_word(word: &str) -> bool {
    matches!(
        word,
        "copy"
            | "default"
            | "defaults"
            | "error"
            | "errors"
            | "interface"
            | "label"
            | "labels"
            | "language"
            | "locale"
            | "message"
            | "messages"
            | "naming"
            | "response"
            | "responses"
            | "throughout"
            | "ui"
    )
}

fn alternative_word(word: &str) -> bool {
    matches!(
        word,
        "and" | "between" | "either" | "else" | "or" | "versus" | "vs" | "또는" | "아니면" | "혹은"
    )
}

fn supported_locale_token(word: &str) -> bool {
    let normalized = word
        .strip_suffix('로')
        .or_else(|| word.strip_suffix('를'))
        .unwrap_or(word);
    matches!(normalized, "english" | "korean" | "영어" | "한국어")
}

fn plausible_locale_token(word: &str) -> bool {
    locale_language_token(word)
}

fn locale_language_token(word: &str) -> bool {
    let normalized = word
        .strip_suffix('로')
        .or_else(|| word.strip_suffix('를'))
        .unwrap_or(word);
    matches!(
        normalized,
        "arabic"
            | "chinese"
            | "dutch"
            | "french"
            | "german"
            | "italian"
            | "japanese"
            | "portuguese"
            | "polish"
            | "spanish"
            | "english"
            | "korean"
            | "독일어"
            | "스페인어"
            | "아랍어"
            | "영어"
            | "이탈리아어"
            | "일본어"
            | "중국어"
            | "포르투갈어"
            | "폴란드어"
            | "프랑스어"
            | "한국어"
    )
}

fn korean_negative_directive(words: &[&str]) -> bool {
    words.iter().any(|word| {
        ["답변하지", "사용하지", "응답하지", "작성하지", "쓰지"]
            .iter()
            .any(|marker| word.contains(marker))
    }) || (words.contains(&"안")
        && words.iter().any(|word| {
            ["답변", "사용", "응답", "작성", "쓰기"]
                .iter()
                .any(|marker| word.contains(marker))
        }))
}

fn korean_locale_default_fragment(words: &[&str]) -> Option<IntentLocaleHintV2> {
    if korean_negative_directive(words)
        || has_korean_semantic_analysis(words)
        || !has_korean_default_output(words)
    {
        return None;
    }
    let english = words.iter().any(|word| word.starts_with("영어"));
    let korean = words.iter().any(|word| word.starts_with("한국어"));
    match (english, korean) {
        (true, false) => Some(IntentLocaleHintV2::En),
        (false, true) => Some(IntentLocaleHintV2::Ko),
        _ => None,
    }
}

fn korean_default_continuation(words: &[&str]) -> bool {
    !korean_negative_directive(words)
        && !has_korean_semantic_analysis(words)
        && words.first().is_some_and(|word| {
            ["기본", "문구", "이름", "응답", "컨트롤"]
                .iter()
                .any(|marker| word.contains(marker))
        })
        && words
            .iter()
            .any(|word| word.contains("사용") || word.contains("작성"))
}

fn has_korean_default_output(words: &[&str]) -> bool {
    words.iter().any(|word| word.contains("기본"))
        && words.iter().any(|word| {
            ["문구", "이름", "응답"]
                .iter()
                .any(|marker| word.contains(marker))
        })
}

fn has_korean_semantic_analysis(words: &[&str]) -> bool {
    words.iter().any(|word| {
        [
            "classifier",
            "classify",
            "classification",
            "detect",
            "detector",
            "detection",
            "감지",
            "검색",
            "분류",
            "분석",
            "인식",
            "탐지",
        ]
        .iter()
        .any(|marker| word.contains(marker))
    })
}

fn close_directive(
    value: &str,
    words: &[&str],
    inherited_close_scope: bool,
) -> AxisDirective<CloseAuthorizationV2> {
    let disabled = disabled_close_directive(value, words);
    let creator_only = creator_close_directive(value, words, inherited_close_scope)
        && !negative_close_scope(value, words);
    let any_member = any_member_close_directive(value, words, inherited_close_scope)
        && !negative_close_scope(value, words);
    match [disabled, any_member, creator_only]
        .into_iter()
        .filter(|selected| *selected)
        .count()
    {
        0 => AxisDirective::None,
        1 if disabled => AxisDirective::Value(CloseAuthorizationV2::Disabled),
        1 if any_member => AxisDirective::Value(CloseAuthorizationV2::AnyMember),
        1 => AxisDirective::Value(CloseAuthorizationV2::CreatorOnly),
        _ => AxisDirective::Conflict,
    }
}

fn merge_alternative_close_branch(
    directive: AxisDirective<CloseAuthorizationV2>,
    previous: Option<CloseAuthorizationV2>,
    branch: AxisDirective<CloseAuthorizationV2>,
    alternative: bool,
) -> AxisDirective<CloseAuthorizationV2> {
    if !alternative {
        return directive;
    }
    let current = match (directive, branch) {
        (AxisDirective::Conflict, _) | (_, AxisDirective::Conflict) => {
            return AxisDirective::Conflict;
        }
        (AxisDirective::Value(left), AxisDirective::Value(right)) if left != right => {
            return AxisDirective::Conflict;
        }
        (AxisDirective::Value(value), _) | (_, AxisDirective::Value(value)) => Some(value),
        (AxisDirective::None, AxisDirective::None) => None,
    };
    match (previous, current) {
        (Some(left), Some(right)) if left != right => AxisDirective::Conflict,
        (_, Some(value)) => AxisDirective::Value(value),
        _ => AxisDirective::None,
    }
}

fn close_branch_hint(
    value: &str,
    words: &[&str],
    inherited_close_scope: bool,
    alternative: bool,
) -> AxisDirective<CloseAuthorizationV2> {
    let any_member = english_any_member_close(words)
        || (["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"]
            .iter()
            .any(|marker| value.contains(marker))
            && korean_close_permission(value))
        || incomplete_any_member_branch(value, words);
    let creator_only = creator_close_directive(value, words, inherited_close_scope)
        || (alternative && inherited_close_scope && starts_with_creator_branch(value, words))
        || ((inherited_close_scope || direct_close_scope(value, words))
            && incomplete_creator_branch(value, words));
    let shared_inline_scope = alternative && direct_close_scope(value, words);
    let any_member = any_member
        || (shared_inline_scope
            && has_alternative_connector(value, words)
            && english_member_scope(words).is_some()
            && has_creator_actor(value, words));
    let creator_only = creator_only
        || (shared_inline_scope
            && has_alternative_connector(value, words)
            && has_creator_actor(value, words)
            && has_any_member_actor(value, words));
    match (any_member, creator_only) {
        (true, true) => AxisDirective::Conflict,
        (true, false) => AxisDirective::Value(CloseAuthorizationV2::AnyMember),
        (false, true) => AxisDirective::Value(CloseAuthorizationV2::CreatorOnly),
        (false, false) => AxisDirective::None,
    }
}

fn incomplete_any_member_branch(value: &str, words: &[&str]) -> bool {
    let english = english_member_scope(words).is_some_and(|(start, end)| {
        end == words.len()
            && (start == 0
                || words[..start]
                    .first()
                    .is_some_and(|word| matches!(*word, "allow" | "enable" | "let")))
    });
    let trimmed = value.trim();
    let korean = ["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"].contains(&trimmed);
    english || korean
}

fn incomplete_creator_branch(value: &str, words: &[&str]) -> bool {
    let english = words == ["only", "the", "room", "creator"]
        || words == ["only", "room", "creator"]
        || words == ["only", "the", "creator"]
        || words == ["only", "creator"];
    let trimmed = value.trim();
    let korean = ["만든 사람만", "방을 만든 사람만", "방 생성자만", "방장만"].contains(&trimmed);
    english || korean
}

fn starts_with_creator_branch(value: &str, words: &[&str]) -> bool {
    words.starts_with(&["only", "the", "room", "creator"])
        || words.starts_with(&["only", "room", "creator"])
        || words.starts_with(&["only", "the", "creator"])
        || words.starts_with(&["only", "creator"])
        || ["만든 사람만", "방 생성자만", "방장만"]
            .iter()
            .any(|marker| value.starts_with(marker))
}

fn has_creator_actor(value: &str, words: &[&str]) -> bool {
    contains_sequence(words, &["only", "the", "room", "creator"])
        || contains_sequence(words, &["only", "room", "creator"])
        || contains_sequence(words, &["only", "the", "creator"])
        || contains_sequence(words, &["only", "creator"])
        || ["만든 사람만", "방 생성자만", "방장만"]
            .iter()
            .any(|marker| value.contains(marker))
}

fn has_any_member_actor(value: &str, words: &[&str]) -> bool {
    english_member_scope(words).is_some()
        || ["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"]
            .iter()
            .any(|marker| value.contains(marker))
}

fn disabled_close_directive(value: &str, words: &[&str]) -> bool {
    let direct_words = strip_directive_prefixes(words);
    let english = direct_words.starts_with(&["leave", "closing", "disabled"])
        || direct_words.starts_with(&["leave", "room", "closing", "disabled"])
        || direct_words.starts_with(&["leave", "the", "close", "button", "disabled"])
        || direct_words.starts_with(&["leave", "close", "button", "disabled"])
        || direct_words.starts_with(&["keep", "closing", "disabled"])
        || direct_words.starts_with(&["keep", "room", "closing", "disabled"])
        || direct_words.starts_with(&["keep", "the", "close", "button", "disabled"])
        || direct_words.starts_with(&["keep", "close", "button", "disabled"])
        || direct_words.starts_with(&["the", "close", "button", "must", "remain", "disabled"])
        || direct_words.starts_with(&["the", "close", "button", "should", "remain", "disabled"])
        || direct_words.starts_with(&["closing", "is", "disabled"])
        || direct_words.starts_with(&["room", "closing", "is", "disabled"])
        || direct_words.starts_with(&["never", "enable", "closing"])
        || direct_words.starts_with(&["never", "enable", "room", "closing"])
        || direct_words.starts_with(&["do", "not", "add", "room", "closing"])
        || matches!(
            direct_words,
            ["don't" | "don’t" | "dont", "add", "room", "closing", ..]
        )
        || (direct_words
            .first()
            .is_some_and(|word| matches!(*word, "disable" | "omit" | "remove"))
            && (has_close_control(direct_words)
                || contains_sequence(direct_words, &["room", "closing"])))
        || (has_close_control(direct_words)
            && (direct_words.starts_with(&["do", "not", "add"])
                || direct_words.starts_with(&["do", "not", "enable"])
                || direct_words.starts_with(&["do", "not", "include"])
                || direct_words.starts_with(&["do", "not", "use"])
                || matches!(
                    direct_words,
                    [
                        "don't" | "don’t" | "dont",
                        "add" | "enable" | "include" | "use",
                        ..
                    ]
                )))
        || (direct_words.starts_with(&["do", "not", "allow", "anyone", "to", "close"])
            && direct_close_scope(value, direct_words))
        || (direct_words.starts_with(&["do", "not", "let", "anyone", "close"])
            && direct_close_scope(value, direct_words));
    let korean_actor = korean_close_actor_scope(value);
    let korean = value.contains("닫기")
        && [
            "넣지 마",
            "넣지마",
            "비활성화해",
            "사용하지 마",
            "사용하지마",
            "추가하지 마",
            "추가하지마",
            "꺼둬",
            "꺼 둬",
            "빼줘",
            "빼 줘",
        ]
        .iter()
        .any(|marker| value.contains(marker))
        && !korean_actor;
    english || korean
}

fn creator_close_directive(value: &str, words: &[&str], inherited_close_scope: bool) -> bool {
    let close_scope = direct_close_scope(value, words) || inherited_close_scope;
    let english = english_creator_close(words);
    let korean = ["만든 사람만", "방 생성자만", "방장만"]
        .iter()
        .any(|marker| value.contains(marker))
        && korean_close_permission(value);
    close_scope && (english || korean)
}

fn english_creator_close(words: &[&str]) -> bool {
    if contains_sequence(
        words,
        &[
            "close", "button", "must", "work", "only", "for", "the", "person", "who", "created",
        ],
    ) || (has_any(words, &["creator-only"])
        && has_any(words, &["close", "closing"])
        && has_any(words, &["allow", "make", "require"]))
        || (contains_sequence(words, &["only", "by", "the", "room", "creator"])
            && has_close_control(words)
            && has_any(words, &["can", "may", "must", "should"]))
    {
        return true;
    }
    let Some((start, end)) = english_creator_scope(words) else {
        return false;
    };
    let before = &words[..start];
    let after = &words[end..];
    let direct_permission = start == 0
        && (matches!(
            after,
            ["can" | "may" | "must" | "should", "close", ..]
                | ["should", "be", "able", "to", "close", ..]
                | ["is", "allowed", "to", "close", ..]
                | ["to", "close", ..]
        ) || (matches!(after, ["can" | "may" | "must" | "should", "use", ..])
            && has_close_control(after))
            || (after.starts_with(&["should", "be", "able", "to", "use"])
                && has_close_control(after))
            || (after.starts_with(&["is", "allowed", "to", "use"]) && has_close_control(after)));
    let let_permission = before.first() == Some(&"let")
        && (after.starts_with(&["close"])
            || (after.starts_with(&["use"]) && has_close_control(after)));
    let allow_permission = before.first() == Some(&"allow")
        && (after.starts_with(&["to", "close"])
            || (after.starts_with(&["to", "use"]) && has_close_control(after)));
    direct_permission || let_permission || allow_permission
}

fn english_creator_scope(words: &[&str]) -> Option<(usize, usize)> {
    for (index, window) in words.windows(7).enumerate() {
        if window == ["the", "room", "creator", "and", "no", "one", "else"] {
            return Some((index, index.saturating_add(7)));
        }
    }
    for (index, window) in words.windows(4).enumerate() {
        if window == ["the", "room", "creator", "alone"] {
            return Some((index, index.saturating_add(4)));
        }
    }
    for (index, window) in words.windows(7).enumerate() {
        if window == ["only", "the", "person", "who", "created", "the", "room"] {
            return Some((index, index.saturating_add(7)));
        }
    }
    for (index, window) in words.windows(6).enumerate() {
        if window == ["only", "person", "who", "created", "the", "room"] {
            return Some((index, index.saturating_add(6)));
        }
    }
    for (index, window) in words.windows(4).enumerate() {
        if window == ["only", "the", "room", "creator"] {
            return Some((index, index.saturating_add(4)));
        }
    }
    for (index, window) in words.windows(3).enumerate() {
        if window == ["only", "room", "creator"] || window == ["only", "the", "creator"] {
            return Some((index, index.saturating_add(3)));
        }
    }
    for (index, window) in words.windows(2).enumerate() {
        if window == ["only", "creator"] {
            return Some((index, index.saturating_add(2)));
        }
    }
    None
}

fn any_member_close_directive(value: &str, words: &[&str], inherited_close_scope: bool) -> bool {
    let close_scope = direct_close_scope(value, words) || inherited_close_scope;
    let english = english_any_member_close(words);
    let korean = ["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"]
        .iter()
        .any(|marker| value.contains(marker))
        && korean_close_permission(value);
    close_scope && (english || korean)
}

fn english_any_member_close(words: &[&str]) -> bool {
    let Some((start, end)) = english_member_scope(words) else {
        return false;
    };
    let before = &words[..start];
    let after = &words[end..];
    let direct_permission = start == 0
        && (matches!(
            after,
            ["can" | "may" | "must" | "should", "close", ..]
                | ["should", "be", "able", "to", "close", ..]
                | ["is", "allowed", "to", "close", ..]
        ) || (matches!(after, ["can" | "may" | "must" | "should", "use", ..])
            && has_close_control(after))
            || (after.starts_with(&["should", "be", "able", "to", "use"])
                && has_close_control(after)));
    let passive_permission = before.starts_with(&["the", "close", "button"])
        && before.ends_with(&["be", "used", "by"])
        && has_any(before, &["can", "may", "must", "should"]);
    let let_permission = before.first() == Some(&"let")
        && (after.starts_with(&["close"])
            || (after.starts_with(&["use"]) && has_close_control(after)));
    let allow_permission = before.first() == Some(&"allow")
        && (after.starts_with(&["to", "close"])
            || (after.starts_with(&["to", "use"]) && has_close_control(after)));
    let enabled_control = before.first() == Some(&"enable")
        && before.last() == Some(&"for")
        && has_any(before, &["close", "closing"]);
    let working_control =
        before.ends_with(&["work", "for"]) && has_any(before, &["close", "closing"]);
    direct_permission
        || passive_permission
        || let_permission
        || allow_permission
        || enabled_control
        || working_control
}

fn english_member_scope(words: &[&str]) -> Option<(usize, usize)> {
    if let Some(index) = words.iter().position(|word| *word == "anyone") {
        return Some((index, index.saturating_add(1)));
    }
    for (index, window) in words.windows(3).enumerate() {
        if matches!(
            window,
            ["any" | "all" | "every", "room", "member" | "members"]
        ) {
            return Some((index, index.saturating_add(3)));
        }
    }
    for (index, window) in words.windows(2).enumerate() {
        if matches!(window, ["any" | "all" | "every", "member" | "members"]) {
            return Some((index, index.saturating_add(2)));
        }
    }
    None
}

fn korean_close_permission(value: &str) -> bool {
    [
        "닫게 해",
        "닫게해",
        "닫을 수 있게 해",
        "닫을 수 있게해",
        "닫을 수 있어야 해",
        "닫을 수 있어",
        "닫아도 돼",
        "닫아도 된다",
        "닫기 버튼을 사용할 수 있게 해",
        "닫기 버튼을 사용할 수 있게해",
        "닫기 버튼을 사용할 수 있어야 해",
        "닫기 버튼 사용을 허용",
        "닫기 버튼을 사용하게 해",
        "닫기 버튼을 사용하게해",
        "방 닫기를 허용",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn korean_close_actor_scope(value: &str) -> bool {
    [
        "만든 사람",
        "모든 방 참가자",
        "모든 참가자",
        "모든 멤버",
        "누구나",
        "방 생성자",
        "방장",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn direct_close_scope(value: &str, words: &[&str]) -> bool {
    let close_control = has_close_control(words);
    let direct_control_permission = contains_sequence(words, &["enable", "close", "for"])
        || contains_sequence(words, &["enable", "closing", "for"]);
    let close_room = [
        &["close", "room"][..],
        &["close", "a", "room"],
        &["close", "the", "room"],
        &["close", "that", "room"],
        &["close", "this", "room"],
        &["room", "closing"],
    ]
    .iter()
    .any(|sequence| contains_sequence(words, sequence));
    let room_actor = contains_sequence(words, &["room", "creator"])
        || contains_sequence(words, &["room", "member"])
        || contains_sequence(words, &["person", "who", "created"])
        || english_creator_scope(words).is_some()
        || english_member_scope(words).is_some();
    let implicit_room = room_actor
        && (contains_sequence(words, &["close", "it"])
            || words.last().is_some_and(|word| *word == "close"));
    let english = close_control || direct_control_permission || close_room || implicit_room;
    let korean_close = value.contains("닫기")
        || value.contains("닫아")
        || value.contains("닫을")
        || value.contains("닫는");
    let korean_business_target = ["메시지", "게시물", "스레드", "이슈", "티켓"]
        .iter()
        .any(|target| value.contains(target));
    let korean_room_target = ["방 닫", "방닫", "방을 닫"]
        .iter()
        .any(|target| value.contains(target));
    let korean = korean_close
        && (korean_room_target
            || value.contains("닫기 버튼")
            || value.contains("닫기 기능")
            || value.contains("닫기 컨트롤")
            || (contains_korean_room_token(value) && !korean_business_target));
    english || korean
}

fn contains_korean_room_token(value: &str) -> bool {
    value.match_indices('방').any(|(start, _)| {
        let suffix = value
            .get(start.saturating_add('방'.len_utf8())..)
            .unwrap_or_default();
        suffix.is_empty()
            || suffix
                .chars()
                .next()
                .is_some_and(|character| !character.is_alphanumeric())
            || KOREAN_TARGET_PARTICLES
                .iter()
                .any(|particle| suffix.starts_with(particle))
    })
}

fn has_close_control(words: &[&str]) -> bool {
    words.windows(2).any(|window| {
        matches!(
            window,
            ["close" | "closing", "button" | "control" | "feature"]
        )
    })
}

fn negative_close_scope(value: &str, words: &[&str]) -> bool {
    has_any(words, &["disabled", "never", "without"])
        || contains_sequence(words, &["do", "not"])
        || ["can", "may", "must", "should"]
            .iter()
            .any(|modal| contains_sequence(words, &[*modal, "not"]))
        || has_any(words, &["cannot"])
        || has_any(words, &["don't", "don’t", "dont"])
        || [
            "금지",
            "닫지 못",
            "닫을 수 없",
            "사용하지",
            "사용 못",
            "사용할 수 없",
        ]
        .iter()
        .any(|marker| value.contains(marker))
}

fn unsupported_close_request(value: &str, words: &[&str]) -> bool {
    let business_target = has_any(
        words,
        &[
            "issue", "issues", "message", "messages", "post", "posts", "thread", "threads",
            "ticket", "tickets",
        ],
    ) || ["메시지", "게시물", "스레드", "이슈", "티켓"]
        .iter()
        .any(|target| value.contains(target));
    if business_target && !direct_close_scope(value, words) {
        return false;
    }
    if korean_close_non_normative(value) {
        return false;
    }
    let close_axis = direct_close_scope(value, words)
        || has_any(words, &["close", "closing"])
        || value.contains("닫기")
        || value.contains("닫을");
    let direct_words = strip_directive_prefixes(words);
    let direct_disable = direct_words
        .first()
        .is_some_and(|word| matches!(*word, "disable" | "enable" | "omit" | "remove"))
        && direct_close_scope(value, direct_words);
    let direct_creator_only = direct_words
        .first()
        .is_some_and(|word| matches!(*word, "make" | "require"))
        && has_any(direct_words, &["creator-only"])
        && has_close_control(direct_words);
    let direct_policy = direct_disable || direct_creator_only;
    let policy_language = direct_policy
        || unresolved_creator_close_policy(words)
        || unresolved_any_member_close_policy(words)
        || (unknown_close_actor(value, words) && normative_close_permission(words))
        || (english_member_scope(words).is_some()
            && negative_close_scope(value, words)
            && direct_close_scope(value, words))
        || korean_close_permission(value)
        || (korean_close_actor_scope(value) && negative_close_scope(value, words));
    close_axis && policy_language
}

fn korean_close_non_normative(value: &str) -> bool {
    [
        "감지",
        "분류",
        "알림을 보내",
        "없을 때",
        "있는지",
        "있을 때",
        "탐지",
        "확인",
    ]
    .iter()
    .any(|marker| value.contains(marker))
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

fn unresolved_creator_close_policy(words: &[&str]) -> bool {
    let creator_end = english_creator_scope(words)
        .map(|(_, end)| end)
        .or_else(|| {
            words.windows(2).enumerate().find_map(|(index, window)| {
                (window == ["room", "creator"]).then_some(index.saturating_add(2))
            })
        });
    let Some(creator_end) = creator_end else {
        return false;
    };
    let after = &words[creator_end..];
    matches!(
        after,
        ["can" | "may" | "must" | "should", "close", ..]
            | ["can" | "may" | "must" | "should", "not", "close", ..]
            | ["can" | "may" | "must" | "should", "not", "use", ..]
    ) || (matches!(after, ["can" | "may" | "must" | "should", "use", ..])
        && has_close_control(after))
}

fn unresolved_any_member_close_policy(words: &[&str]) -> bool {
    let Some((_, member_end)) = english_member_scope(words) else {
        return false;
    };
    normative_close_permission(&words[member_end..]) && direct_close_scope("", words)
}

fn normative_close_permission(words: &[&str]) -> bool {
    has_any(words, &["allow", "enable", "let"])
        || words.iter().enumerate().any(|(index, word)| {
            matches!(*word, "can" | "may" | "must" | "should")
                && words.get(index.saturating_add(1)).is_some_and(|next| {
                    matches!(*next, "close" | "use")
                        || (*next == "not"
                            && words
                                .get(index.saturating_add(2))
                                .is_some_and(|tail| matches!(*tail, "close" | "use")))
                })
        })
        || contains_sequence(words, &["is", "allowed", "to", "close"])
        || contains_sequence(words, &["is", "allowed", "to", "use"])
}

fn inline_locale_alternative(value: &str, words: &[&str]) -> bool {
    let supported_korean_pair = value.contains("영어") && value.contains("한국어");
    let supported_ascii_pair = value.contains("english")
        && value.contains("korean")
        && (value.contains('/') || value.contains(" vs ") || value.contains(" & "));
    let candidates = locale_candidate_tokens(words);
    (has_alternative_connector(value, words) || slash_locale_alternative(value))
        && (candidates.len() > 1 || supported_korean_pair || supported_ascii_pair)
}

fn slash_locale_alternative(value: &str) -> bool {
    value.match_indices('/').any(|(index, _)| {
        let left = value
            .get(..index)
            .unwrap_or_default()
            .rsplit(|character: char| !character.is_alphanumeric())
            .next()
            .unwrap_or_default();
        let right = value
            .get(index.saturating_add(1)..)
            .unwrap_or_default()
            .split(|character: char| !character.is_alphanumeric())
            .next()
            .unwrap_or_default();
        locale_language_token(left) && locale_language_token(right)
    })
}

fn inline_close_alternative(value: &str, words: &[&str]) -> bool {
    if (!has_alternative_connector(value, words) && !value.contains('/'))
        || !direct_close_scope(value, words)
    {
        return false;
    }
    let disabled = disabled_close_directive(value, words)
        || (direct_close_scope(value, words) && has_any(words, &["disabled"]));
    let any_member = has_any_member_actor(value, words);
    let creator_only = has_creator_actor(value, words);
    let unsupported_actor = unknown_close_actor(value, words);
    [disabled, any_member, creator_only, unsupported_actor]
        .into_iter()
        .filter(|branch| *branch)
        .count()
        > 1
}

fn unknown_close_actor(value: &str, words: &[&str]) -> bool {
    has_any(
        words,
        &[
            "admin",
            "admins",
            "administrator",
            "administrators",
            "guest",
            "guests",
            "host",
            "hosts",
            "moderator",
            "moderators",
            "owner",
            "owners",
            "role",
            "roles",
            "subscriber",
            "subscribers",
            "user",
            "users",
        ],
    ) || [
        "게스트",
        "관리자",
        "구독자",
        "운영자",
        "특정 역할",
        "호스트",
    ]
    .iter()
    .any(|actor| value.contains(actor))
}

fn unsupported_close_alternative_branch(value: &str, words: &[&str]) -> bool {
    has_any(
        words,
        &[
            "admin",
            "admins",
            "administrator",
            "administrators",
            "creator",
            "guest",
            "guests",
            "host",
            "hosts",
            "moderator",
            "moderators",
            "owner",
            "owners",
            "role",
            "roles",
            "user",
            "users",
        ],
    ) || has_any(words, &["close", "closing", "permission", "permissions"])
        || korean_close_actor_scope(value)
}

fn unsupported_close_modifier(value: &str, words: &[&str]) -> bool {
    let restriction = has_any(
        words,
        &[
            "approval",
            "after",
            "before",
            "confirmation",
            "during",
            "except",
            "excluding",
            "if",
            "locked",
            "unless",
            "until",
            "when",
            "whenever",
            "while",
        ],
    ) || contains_sequence(words, &["subject", "to"])
        || value.contains("승인")
        || value.contains("제외")
        || value.contains("확인 후");
    let scoped_target = words.windows(2).any(|window| {
        matches!(window[0], "at" | "on")
            && matches!(
                window[1],
                "night" | "weekdays" | "weekends" | "working-hours"
            )
    });
    direct_close_scope(value, words)
        && (restriction || scoped_target)
        && (normative_close_permission(words) || korean_close_permission(value))
}

fn unsupported_connected_close_modifier(value: &str, words: &[&str]) -> bool {
    unknown_close_actor(value, words)
        && (normative_close_permission(words)
            || has_any(words, &["deny", "disable", "exclude", "forbid", "remove"]))
}

fn connected_close_restriction(
    value: &str,
    directive_words: &[&str],
    continuation: Option<&str>,
) -> bool {
    let explicit_prefix = directive_words.starts_with(&["except"])
        || directive_words.starts_with(&["excluding"])
        || directive_words.starts_with(&["unless"])
        || value.starts_with("단 ");
    let elliptical_negative = unknown_close_actor(value, directive_words)
        && matches!(
            directive_words,
            [_, "cannot"] | ["but", _, "cannot"] | [_, "may", "not"] | ["but", _, "may", "not"]
        );
    let actor_exclusion =
        matches!(
            directive_words,
            [
                "except" | "excluding",
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "host"
                    | "hosts"
                    | "moderator"
                    | "moderators"
                    | "owner"
                    | "owners"
                    | "subscriber"
                    | "subscribers"
                    | "user"
                    | "users"
            ] | [
                "except" | "excluding",
                "the",
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "host"
                    | "hosts"
                    | "moderator"
                    | "moderators"
                    | "owner"
                    | "owners"
                    | "subscriber"
                    | "subscribers"
                    | "user"
                    | "users"
            ]
        ) || value.starts_with("단 ") && value.contains("제외") && directive_words.len() <= 4;
    let actor_exclusion_continues_close = continuation.is_none_or(|continuation| {
        let continuation_words = words(continuation);
        direct_close_scope(continuation, &continuation_words)
            || has_any(
                &continuation_words,
                &["approval", "cannot", "confirmation", "locked", "unless"],
            )
            || continuation.contains("승인")
            || continuation.contains("확인 후")
    });
    elliptical_negative
        || explicit_prefix
            && (actor_exclusion && actor_exclusion_continues_close
                || has_any(
                    directive_words,
                    &["approval", "cannot", "confirmation", "locked", "unless"],
                )
                || value.contains("승인")
                || value.contains("확인 후"))
}

fn unsupported_close_condition_continuation(value: &str) -> bool {
    let words = words(value);
    has_any(
        &words,
        &[
            "active",
            "approval",
            "archived",
            "confirmation",
            "ends",
            "locked",
            "scheduled",
            "weekdays",
            "weekends",
        ],
    ) || value.contains("승인")
        || value.contains("잠겨")
        || value.contains("확인")
}

fn has_alternative_connector(value: &str, words: &[&str]) -> bool {
    [" or ", " versus ", " vs "]
        .iter()
        .any(|marker| value.contains(marker))
        || words
            .first()
            .is_some_and(|word| matches!(*word, "or" | "versus" | "vs"))
        || value.contains("and/or")
        || value.contains(" & ")
        || (value.contains("between ") && has_any(words, &["and"]))
        || value.contains("english/korean")
        || value.contains("korean/english")
        || [" 또는 ", " 혹은 ", " 아니면 ", " 중 하나"]
            .iter()
            .any(|marker| value.contains(marker))
}

fn starts_alternative_prefix(value: &str) -> bool {
    value.starts_with("or ")
        || value.starts_with("or else ")
        || value.starts_with("versus ")
        || value.starts_with("vs ")
        || value.starts_with("또는 ")
        || value.starts_with("아니면 ")
        || value.starts_with("혹은 ")
}

fn closed_axis_detector_context(value: &str) -> bool {
    let lexical_context = [
        " as detector input",
        " as classifier input",
        " as an example",
        " for classification",
        " language detection",
        " condition to detect",
        " phrase to detect",
        " policy description",
        "detector where ",
        "detects whether ",
        "detects if ",
        "달라는 요청",
        "달라고 요청",
        "요청을 감지",
        "요청을 기록",
    ]
    .iter()
    .any(|marker| value.contains(marker));
    let user_interface_context = ["customer-facing", "end-user", "user-facing"]
        .iter()
        .any(|marker| value.contains(marker))
        && [
            "copy",
            "interface",
            "label",
            "labels",
            "panel",
            "screen",
            "settings",
            "ui",
        ]
        .iter()
        .any(|marker| value.contains(marker));
    let training_context = !user_interface_context
        && (value.contains("classifier") || value.contains("detector"))
        && (value.contains(" to train ")
            || value.contains(" training")
            || value.contains(" in the classifier")
            || value.contains(" in the detector"));
    lexical_context
        || training_context
        || value.starts_with("detect ")
        || value.starts_with("classify ")
        || value.starts_with("record the phrase ")
}

fn opens_closed_axis_detector_scope(value: &str) -> bool {
    [
        "audit automation that records",
        "audit automation that detects",
        "automation that records",
        "automation that detects",
        "detector that records",
        "detector that detects",
    ]
    .iter()
    .any(|marker| value.contains(marker))
        || value == "detect"
        || value == "classify"
        || value == "record the phrase"
        || value.starts_with("detect ")
        || value.starts_with("classify ")
        || value.starts_with("record the phrase ")
}

fn starts_closed_axis_imperative(value: &str) -> bool {
    let words = words(value);
    let words = strip_directive_prefixes(&words);
    matches!(
        words,
        [
            "answer"
                | "disable"
                | "enable"
                | "keep"
                | "leave"
                | "omit"
                | "remove"
                | "reply"
                | "respond"
                | "set"
                | "use"
                | "write",
            ..
        ]
    ) || [
        "기본 문구",
        "닫기 기능",
        "닫기 버튼",
        "로 해줘",
        "사용해",
        "설정해",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn correction_directive(value: &str) -> bool {
    [
        "actually",
        "correction",
        "instead",
        "no",
        "rather",
        "아니",
        "대신",
        "실제로",
        "정정",
        "정정하면",
        "정정해서",
    ]
    .iter()
    .any(|prefix| {
        value.strip_prefix(prefix).is_some_and(|tail| {
            tail.chars().next().is_some_and(|character| {
                character.is_whitespace() || matches!(character, ',' | ':' | '–' | '—')
            })
        })
    })
}

fn standalone_correction(value: &str) -> bool {
    matches!(
        value.trim_matches(|character: char| {
            matches!(character, ',' | ':') || character.is_whitespace()
        }),
        "actually"
            | "correction"
            | "instead"
            | "no"
            | "rather"
            | "아니"
            | "대신"
            | "실제로"
            | "정정"
            | "정정하면"
            | "정정해서"
    )
}

fn strip_directive_prefixes<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    while words.first().is_some_and(|word| {
        matches!(
            *word,
            "actually"
                | "correction"
                | "else"
                | "instead"
                | "no"
                | "or"
                | "please"
                | "rather"
                | "아니"
                | "대신"
                | "실제로"
                | "정정"
                | "정정하면"
                | "정정해서"
        )
    }) {
        words = &words[1..];
    }
    words
}

fn words(value: &str) -> Vec<&str> {
    let words = value
        .split(|character: char| {
            !character.is_alphanumeric() && !matches!(character, '-' | '\'' | '\u{2019}' | '_')
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    record_closed_axis_work(words.len());
    words
}

fn has_any(words: &[&str], candidates: &[&str]) -> bool {
    record_closed_axis_work(words.len());
    words.iter().any(|word| candidates.contains(word))
}

fn contains_sequence(words: &[&str], sequence: &[&str]) -> bool {
    record_closed_axis_work(words.len());
    !sequence.is_empty()
        && words
            .windows(sequence.len())
            .any(|window| window == sequence)
}

#[cfg(test)]
fn record_closed_axis_work(amount: usize) {
    CLOSED_AXIS_WORK.with(|work| work.set(work.get().saturating_add(amount)));
}

#[cfg(not(test))]
fn record_closed_axis_work(_amount: usize) {}

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

    fn measured_work(repetitions: usize) -> usize {
        CLOSED_AXIS_WORK.with(|work| work.set(0));
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
        CLOSED_AXIS_WORK.with(Cell::get)
    }

    fn measured_input_work(repetitions: usize) -> usize {
        let value = format!("use korean defaults {}", "on desktop ".repeat(repetitions));
        CLOSED_AXIS_WORK.with(|work| work.set(0));
        let mut accumulator = ClosedAxesAccumulator::default();
        accumulator.observe(&value, UnquotedGroundingLink::Detached, None, None);
        let _ = accumulator.finish();
        CLOSED_AXIS_WORK.with(Cell::get)
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
        ] {
            assert_eq!(grounded_closed_axis_restatement(value).1, Some(expected));
        }
    }
}
