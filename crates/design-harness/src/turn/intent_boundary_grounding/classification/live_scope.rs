use super::gate_control::marker_has_boundaries;
use super::vocabulary::*;
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
    let mut following = value[alias_end..].split_whitespace();
    let Some(resource) = following.next() else {
        return false;
    };
    LIVE_MUTABLE_RESOURCE_TARGETS.contains(&resource)
        && !following
            .next()
            .is_some_and(|word| LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&word))
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
