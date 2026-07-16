use super::lexical::strip_repeated_prefixes;
use super::patterns::*;
use crate::turn::intent_metalinguistic_scope::first_ascii_word_index;

pub(super) struct BuildDirective {
    pub(super) targets: Vec<RequestTarget>,
}

pub(super) enum HoldDirective {
    Global,
    Target(RequestTarget),
    Scoped,
}

pub(super) enum RequestDirective {
    Build(BuildDirective),
    Discussion,
    Hold(HoldDirective),
}

fn explicit_build(unit: &str, continuation: Option<&str>) -> Option<BuildDirective> {
    explicit_english_build(unit).or_else(|| explicit_korean_build(unit, continuation))
}

pub(super) fn request_directive(
    unit: &str,
    continuation: Option<&str>,
) -> Option<RequestDirective> {
    if explicit_discussion(unit) {
        Some(RequestDirective::Discussion)
    } else if let Some(directive) = english_build_hold_directive(unit) {
        Some(directive)
    } else if let Some(directive) = korean_build_hold_directive(unit) {
        Some(directive)
    } else {
        explicit_build(unit, continuation).map(RequestDirective::Build)
    }
}

fn explicit_english_build(unit: &str) -> Option<BuildDirective> {
    if ENGLISH_METALINGUISTIC_PREDICATES
        .iter()
        .any(|predicate| unit.contains(predicate))
    {
        return None;
    }
    let mut value = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    if let Some(tail) = ENGLISH_POLITE_BUILD_PREFIXES
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
    {
        value = strip_repeated_prefixes(tail, ENGLISH_REQUEST_WRAPPERS);
    }
    if let Some(targets) = ENGLISH_BUILD_PREFIXES.iter().find_map(|prefix| {
        value
            .strip_prefix(prefix)
            .map(direct_english_build_targets)
            .filter(|targets| !targets.is_empty())
    }) {
        return Some(BuildDirective { targets });
    }
    (value.starts_with("i want this designed now") || value.starts_with("i want this built now"))
        .then_some(BuildDirective {
            targets: Vec::new(),
        })
}

fn explicit_korean_build(unit: &str, continuation: Option<&str>) -> Option<BuildDirective> {
    let verb = KOREAN_BUILD_SUFFIXES
        .iter()
        .find_map(|suffix| {
            unit.ends_with(suffix)
                .then_some(unit.len().saturating_sub(suffix.len()))
        })
        .or_else(|| {
            KOREAN_COMPOUND_BUILD_MARKERS.iter().find_map(|marker| {
                unit.find(marker).and_then(|position| {
                    let tail = unit[position.saturating_add(marker.len())..].trim_start();
                    KOREAN_COMPOUND_CONTINUATION_PREFIXES
                        .iter()
                        .any(|prefix| tail.starts_with(prefix))
                        .then_some(position)
                })
            })
        })
        .or_else(|| {
            continuation
                .filter(|tail| {
                    KOREAN_COMPOUND_CONTINUATION_PREFIXES
                        .iter()
                        .any(|prefix| tail.starts_with(prefix))
                })
                .and_then(|_| {
                    KOREAN_SPLIT_COMPOUND_BUILD_SUFFIXES
                        .iter()
                        .find_map(|suffix| unit.strip_suffix(suffix).map(|head| head.len()))
                })
        })?;
    let targets = direct_korean_build_targets(unit.get(..verb)?.trim());
    (!targets.is_empty()).then_some(BuildDirective { targets })
}

fn explicit_discussion(unit: &str) -> bool {
    let english = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    let english = english
        .strip_prefix("this is ")
        .or_else(|| english.strip_prefix("for now "))
        .or_else(|| english.strip_prefix("for now, "))
        .unwrap_or(english);
    let english_discussion = ENGLISH_DISCUSSION_DIRECTIVES.contains(&english)
        || ENGLISH_DRAFT_HOLD_DIRECTIVES.contains(&english);
    let korean_discussion = KOREAN_DISCUSSION_SUFFIXES
        .iter()
        .any(|suffix| unit.ends_with(suffix))
        || KOREAN_DRAFT_HOLD_DIRECTIVES.contains(&unit);
    english_discussion || korean_discussion
}

fn english_build_hold_directive(unit: &str) -> Option<RequestDirective> {
    let english = strip_repeated_prefixes(unit, ENGLISH_REQUEST_WRAPPERS);
    let tail = ENGLISH_BUILD_HOLD_PREFIXES
        .iter()
        .find_map(|prefix| english.strip_prefix(prefix))?
        .trim();
    let subject = tail
        .strip_suffix(" for now")
        .or_else(|| tail.strip_suffix(" yet"))
        .or_else(|| (tail == "for now" || tail == "yet").then_some(""))
        .unwrap_or(tail);
    let subject = strip_english_article(subject.trim());
    if subject.is_empty() || matches!(subject, "anything" | "it" | "that" | "this") {
        return Some(RequestDirective::Hold(HoldDirective::Global));
    }
    if let Some(target) = terminal_english_target(subject) {
        return Some(RequestDirective::Hold(HoldDirective::Target(target)));
    }
    Some(RequestDirective::Hold(HoldDirective::Scoped))
}

fn korean_build_hold_directive(unit: &str) -> Option<RequestDirective> {
    if !KOREAN_NO_BUILD_SUFFIXES
        .iter()
        .any(|suffix| unit.ends_with(suffix))
    {
        return None;
    }
    let verb = KOREAN_NEGATIVE_BUILD_VERBS
        .iter()
        .filter_map(|verb| unit.rfind(verb))
        .max()?;
    let raw_subject =
        strip_repeated_prefixes(unit.get(..verb)?.trim(), &["아니 ", "아니요 ", "대신 "]);
    let subject = strip_korean_hold_markers(raw_subject);
    if subject.is_empty() || matches!(subject, "그건" | "그것은" | "그걸" | "그것을") {
        return Some(RequestDirective::Hold(HoldDirective::Global));
    }
    terminal_korean_target(subject)
        .map(HoldDirective::Target)
        .or(Some(HoldDirective::Scoped))
        .map(RequestDirective::Hold)
}

fn direct_english_build_targets(value: &str) -> Vec<RequestTarget> {
    let barrier = ENGLISH_OBJECT_BARRIERS
        .iter()
        .filter_map(|barrier| value.find(barrier))
        .min();
    let mut targets = ENGLISH_BUILD_TARGETS
        .iter()
        .filter_map(|(word, target)| {
            first_ascii_inflected_word_index(value, word)
                .filter(|position| barrier.is_none_or(|barrier| *position < barrier))
                .map(|position| (position, *target))
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|(position, _)| *position);
    targets
        .into_iter()
        .map(|(_, target)| target)
        .fold(Vec::new(), |mut unique, target| {
            if !unique.contains(&target) {
                unique.push(target);
            }
            unique
        })
}

fn first_ascii_inflected_word_index(value: &str, expected: &str) -> Option<usize> {
    first_ascii_word_index(value, expected).or_else(|| {
        let plural = format!("{expected}s");
        first_ascii_word_index(value, &plural)
    })
}

fn terminal_english_target(subject: &str) -> Option<RequestTarget> {
    ENGLISH_BUILD_TARGETS
        .iter()
        .filter_map(|(word, target)| {
            first_ascii_inflected_word_index(subject, word).and_then(|index| {
                let suffix = &subject[index..];
                (suffix == *word || suffix == format!("{word}s")).then_some((*word, *target))
            })
        })
        .max_by_key(|(word, _)| word.len())
        .map(|(_, target)| target)
}

fn strip_english_article(value: &str) -> &str {
    ["a ", "an ", "the "]
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .unwrap_or(value)
}

fn strip_korean_hold_markers(mut value: &str) -> &str {
    loop {
        let leading = KOREAN_BUILD_HOLD_MARKERS
            .iter()
            .find_map(|marker| value.strip_prefix(marker));
        if let Some(tail) = leading {
            value = tail.trim_start();
            continue;
        }
        let trailing = KOREAN_BUILD_HOLD_MARKERS
            .iter()
            .find_map(|marker| value.strip_suffix(marker));
        if let Some(head) = trailing {
            value = head.trim_end();
            continue;
        }
        return value;
    }
}

fn terminal_korean_target(subject: &str) -> Option<RequestTarget> {
    let subject = ["으로", "에서", "에게", "은", "는", "이", "가", "을", "를"]
        .iter()
        .find_map(|particle| subject.strip_suffix(particle))
        .unwrap_or(subject)
        .trim_end();
    KOREAN_BUILD_TARGETS
        .iter()
        .filter(|(word, _)| subject.ends_with(word))
        .max_by_key(|(word, _)| word.len())
        .map(|(_, target)| *target)
}

fn direct_korean_build_targets(subject: &str) -> Vec<RequestTarget> {
    KOREAN_BUILD_TARGETS
        .iter()
        .filter(|(word, _)| contains_korean_build_target(subject, word))
        .map(|(_, target)| *target)
        .fold(Vec::new(), |mut unique, target| {
            if !unique.contains(&target) {
                unique.push(target);
            }
            unique
        })
}

fn contains_korean_build_target(subject: &str, expected: &str) -> bool {
    subject.match_indices(expected).any(|(start, _)| {
        if expected != "방" {
            return true;
        }
        let suffix = &subject[start.saturating_add(expected.len())..];
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
