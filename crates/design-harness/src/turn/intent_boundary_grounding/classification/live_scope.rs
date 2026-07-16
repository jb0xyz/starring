use super::gate_control::marker_has_boundaries;
use super::vocabulary::*;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static LIVE_DESTINATION_WORK: Cell<usize> = const { Cell::new(0) };
}
pub(in super::super) fn live_weak_context() -> &'static [&'static str] {
    &["live", "discord", "server", "라이브", "디스코드", "서버"]
}

pub(in super::super) fn contains_any(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
}

pub(super) fn contains_bounded_any(value: &str, markers: &[&str]) -> bool {
    bounded_marker_occurrences(value, markers).next().is_some()
}

pub(super) fn has_operational_live_context(value: &str) -> bool {
    bounded_marker_occurrences(value, LIVE_CONTEXT).any(|(start, _)| {
        let (preposition, preceding) = live_context_predecessors(value, start);
        if descriptive_live_context_preposition(preposition) {
            return false;
        }
        if preposition == Some("on")
            && preceding
                .take(6)
                .any(|word| LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&word))
        {
            return false;
        }
        true
    }) || bounded_marker_occurrences(value, LIVE_CONTEXT_ALIASES).any(|(start, end)| {
        let (preposition, preceding) = live_context_predecessors(value, start);
        if descriptive_live_context_preposition(preposition) {
            return false;
        }
        if preposition == Some("on")
            && preceding
                .take(6)
                .any(|word| LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&word))
        {
            return false;
        }
        live_alias_has_mutable_resource(value, end)
            || preposition.is_some_and(|word| {
                matches!(
                    word,
                    "against" | "at" | "from" | "in" | "into" | "on" | "to"
                )
            })
    })
}

pub(super) fn live_context_predecessors(
    value: &str,
    start: usize,
) -> (Option<&str>, impl Iterator<Item = &str>) {
    let mut preceding = value[..start].split_whitespace().rev();
    let mut preposition = preceding.next();
    while preposition.is_some_and(|word| matches!(word, "a" | "an" | "the")) {
        preposition = preceding.next();
    }
    (preposition, preceding)
}

pub(super) fn descriptive_live_context_preposition(preposition: Option<&str>) -> bool {
    preposition.is_some_and(|word| {
        matches!(
            word,
            "about" | "concerning" | "describing" | "for" | "of" | "regarding" | "representing"
        )
    })
}

pub(super) fn live_alias_has_mutable_resource(value: &str, alias_end: usize) -> bool {
    let mut following = value[alias_end..]
        .split_whitespace()
        .map(normalized_live_word);
    let Some(resource) = following.next() else {
        return false;
    };
    LIVE_MUTABLE_RESOURCE_TARGETS.contains(&resource)
        && !following
            .next()
            .is_some_and(|word| LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&word))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum LiveResourceAntecedent {
    Operational,
    Descriptive,
    Unknown,
}

pub(in super::super) fn live_resource_pronoun_continuation(value: &str) -> bool {
    let mut words = value.split_whitespace();
    let action = words.by_ref().find(|word| {
        !matches!(
            *word,
            "also" | "directly" | "immediately" | "just" | "now" | "please" | "then"
        )
    });
    if !action.is_some_and(|action| LIVE_ACTIONS.contains(&action)) {
        return false;
    }
    match words.next() {
        Some("it" | "that" | "them" | "this") => true,
        Some("the") => words
            .next()
            .is_some_and(|word| LIVE_MUTABLE_RESOURCE_TARGETS.contains(&word)),
        _ => false,
    }
}

pub(in super::super) fn live_resource_antecedent(value: &str) -> LiveResourceAntecedent {
    if has_descriptive_live_resource(value) {
        LiveResourceAntecedent::Descriptive
    } else if has_operational_live_context(value) || has_mutable_discord_destination(value) {
        LiveResourceAntecedent::Operational
    } else {
        LiveResourceAntecedent::Unknown
    }
}

fn has_mutable_discord_destination(value: &str) -> bool {
    let words = live_scope_words(value);
    words.iter().enumerate().any(|(discord_index, word)| {
        if *word != "discord" {
            return false;
        }
        record_live_destination_work(1);
        let resource_after = words
            .iter()
            .enumerate()
            .skip(discord_index.saturating_add(1))
            .take(3)
            .find(|(_, word)| LIVE_MUTABLE_RESOURCE_TARGETS.contains(word));
        if let Some((resource_index, _)) = resource_after {
            let destination = words[..discord_index]
                .iter()
                .rev()
                .take(6)
                .find(|word| !closed_live_resource_modifier(word))
                .is_some_and(|word| live_resource_destination_preposition(word));
            if destination
                && words[discord_index.saturating_add(1)..resource_index]
                    .iter()
                    .all(|word| {
                        closed_live_resource_modifier(word)
                            || LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(word)
                    })
            {
                return true;
            }
        }
        let Some((resource_index, _)) = words[..discord_index]
            .iter()
            .enumerate()
            .rev()
            .take(4)
            .find(|(_, word)| LIVE_MUTABLE_RESOURCE_TARGETS.contains(word))
        else {
            return false;
        };
        discord_index.saturating_sub(resource_index) <= 3
            && words[resource_index.saturating_add(1)..discord_index]
                .iter()
                .any(|word| live_resource_destination_preposition(word))
            && words[resource_index.saturating_add(1)..discord_index]
                .iter()
                .all(|word| {
                    closed_live_resource_modifier(word)
                        || live_resource_destination_preposition(word)
                })
    })
}

#[cfg(test)]
fn record_live_destination_work(amount: usize) {
    LIVE_DESTINATION_WORK.with(|work| work.set(work.get().saturating_add(amount)));
}

#[cfg(not(test))]
fn record_live_destination_work(_amount: usize) {}

fn has_descriptive_live_resource(value: &str) -> bool {
    let words = live_scope_words(value);
    let has_live_resource = words.iter().any(|word| {
        *word == "discord"
            || LIVE_MUTABLE_RESOURCE_TARGETS.contains(word)
            || matches!(*word, "live" | "prod" | "production")
    });
    if !has_live_resource {
        return false;
    }
    let first_action = words
        .iter()
        .find(|word| !closed_live_resource_modifier(word))
        .copied();
    if first_action.is_some_and(|word| {
        matches!(
            word,
            "analyze"
                | "analyse"
                | "describe"
                | "design"
                | "discuss"
                | "document"
                | "preview"
                | "simulate"
        )
    }) {
        return true;
    }
    words.windows(2).enumerate().any(|(index, pair)| {
        (live_resource_word(pair[0])
            && (LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&pair[1]) || pair[1] == "design")
            && !description_is_mutable_resource_modifier(&words, index.saturating_add(1)))
            || ((LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&pair[0]) || pair[0] == "design")
                && live_resource_word(pair[1])
                && !description_is_mutable_resource_modifier(&words, index))
    }) || words.iter().enumerate().any(|(description_index, word)| {
        (LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(word) || *word == "design")
            && !description_is_mutable_resource_modifier(&words, description_index)
            && words
                .iter()
                .enumerate()
                .skip(description_index.saturating_add(1))
                .take(5)
                .find(|(_, word)| live_resource_word(word))
                .is_some_and(|(resource_index, _)| {
                    words[description_index.saturating_add(1)..resource_index]
                        .iter()
                        .all(|word| {
                            closed_live_resource_modifier(word)
                                || matches!(*word, "about" | "for" | "of" | "on" | "regarding")
                        })
                })
    })
}

fn description_is_mutable_resource_modifier(words: &[&str], index: usize) -> bool {
    words
        .iter()
        .skip(index.saturating_add(1))
        .take(3)
        .take_while(|word| closed_live_resource_modifier(word))
        .count()
        .checked_add(1)
        .and_then(|offset| words.get(index.saturating_add(offset)))
        .is_some_and(|word| LIVE_MUTABLE_RESOURCE_TARGETS.contains(word))
        || words
            .get(index.saturating_add(1))
            .is_some_and(|word| LIVE_MUTABLE_RESOURCE_TARGETS.contains(word))
}

fn live_resource_word(word: &str) -> bool {
    word == "discord"
        || LIVE_MUTABLE_RESOURCE_TARGETS.contains(&word)
        || matches!(word, "live" | "prod" | "production")
}

fn closed_live_resource_modifier(word: &str) -> bool {
    matches!(
        word,
        "a" | "an" | "actual" | "bot" | "live" | "public" | "the"
    )
}

fn live_resource_destination_preposition(word: &str) -> bool {
    matches!(word, "in" | "into" | "on" | "onto" | "to" | "within")
}

fn live_scope_words(value: &str) -> Vec<&str> {
    value
        .split_whitespace()
        .map(normalized_live_word)
        .filter(|word| !word.is_empty())
        .collect()
}

fn normalized_live_word(word: &str) -> &str {
    word.trim_matches(|character: char| {
        matches!(
            character,
            ',' | '.' | ':' | ';' | '!' | '?' | '。' | '！' | '？' | '；'
        )
    })
}

pub(super) fn bounded_marker_occurrences<'a>(
    value: &'a str,
    markers: &'a [&'a str],
) -> impl Iterator<Item = (usize, usize)> + 'a {
    markers.iter().flat_map(move |marker| {
        value.match_indices(marker).filter_map(|(start, matched)| {
            let end = start.saturating_add(matched.len());
            marker_has_boundaries(value, start, end).then_some((start, end))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured_destination_work(repetitions: usize) -> usize {
        let value = "discord ".repeat(repetitions);
        LIVE_DESTINATION_WORK.with(|work| work.set(0));
        assert!(!has_mutable_discord_destination(&value));
        LIVE_DESTINATION_WORK.with(Cell::get)
    }

    #[test]
    fn mutable_discord_destination_work_scales_linearly() {
        let small = measured_destination_work(1_024);
        let large = measured_destination_work(2_048);
        assert_eq!(small, 1_024);
        assert_eq!(large, small.saturating_mul(2));
    }
}
