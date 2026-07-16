use super::action_polarity::marker_is_negated;
use super::gate_control::marker_has_boundaries;
use super::live_scope::bounded_marker_occurrences;
use super::vocabulary::*;
use super::{word_continuation, ORDINARY_PREFIX_NEGATIONS, PRESERVATION_PREFIX_NEGATIONS};
#[cfg(test)]
use super::{
    MAXIMAL_SECRET_TARGET_WORK, SECRET_ACTION_POLARITY_WORK, UNPROTECTED_SECRET_PREFIX_STEPS,
};
pub(super) fn has_unnegated_unprotected_secret(value: &str) -> bool {
    let action_polarities = secret_action_polarities(value);
    let control_state = UnprotectedSecretControlState::analyze(value);
    let unprotected = source_ordered_bounded_marker_occurrences(value, UNPROTECTED_SECRET);
    let mut action_index = 0usize;
    let mut latest_action = None;
    unprotected.into_iter().any(|(start, end)| {
        let marker = &value[start..end];
        if marker.starts_with("do not ")
            || marker.starts_with("don't ")
            || marker.starts_with("dont ")
            || marker.starts_with("never ")
        {
            return !marker_is_negated(value, start, end);
        }
        while action_polarities
            .get(action_index)
            .is_some_and(|(action_start, _)| *action_start < start)
        {
            latest_action = action_polarities
                .get(action_index)
                .map(|(_, polarity)| *polarity);
            action_index = action_index.saturating_add(1);
        }
        if let Some(unnegated) = latest_action {
            return unnegated;
        }
        !marker_is_negated(value, start, end) && !control_state.clause_negates_before(start)
    })
}

pub(super) fn source_ordered_bounded_marker_occurrences(
    value: &str,
    markers: &[&str],
) -> Vec<(usize, usize)> {
    let mut furthest_end_at_start = vec![None; value.len().saturating_add(1)];
    for (start, end) in bounded_marker_occurrences(value, markers) {
        let slot = &mut furthest_end_at_start[start];
        *slot = Some(slot.map_or(end, |current: usize| current.max(end)));
    }
    furthest_end_at_start
        .into_iter()
        .enumerate()
        .filter_map(|(start, end)| end.map(|end| (start, end)))
        .collect()
}

pub(super) struct UnprotectedSecretControlState<'a> {
    value: &'a str,
    leading_negative_ready_at: Option<usize>,
    preservation_controls: Vec<(&'static str, Option<usize>)>,
}

impl<'a> UnprotectedSecretControlState<'a> {
    fn analyze(value: &'a str) -> Self {
        let trimmed = value.trim_start();
        let trimmed_start = value.len().saturating_sub(trimmed.len());
        let leading_negative_ready_at = ORDINARY_PREFIX_NEGATIONS
            .iter()
            .filter_map(|control| {
                let remainder = trimmed.strip_prefix(control)?;
                remainder
                    .chars()
                    .next()
                    .is_some_and(char::is_whitespace)
                    .then_some(())?;
                remainder
                    .char_indices()
                    .find(|(_, character)| !character.is_whitespace())
                    .map(|(start, character)| {
                        trimmed_start
                            .saturating_add(control.len())
                            .saturating_add(start)
                            .saturating_add(character.len_utf8())
                    })
            })
            .min();
        let preservation_controls = PRESERVATION_PREFIX_NEGATIONS
            .iter()
            .map(|control| {
                (
                    *control,
                    bounded_marker_occurrences(value, &[*control])
                        .next()
                        .map(|(_, end)| end),
                )
            })
            .collect();
        Self {
            value,
            leading_negative_ready_at,
            preservation_controls,
        }
    }

    fn clause_negates_before(&self, before: usize) -> bool {
        #[cfg(test)]
        UNPROTECTED_SECRET_PREFIX_STEPS.with(|steps| {
            steps.set(
                steps
                    .get()
                    .saturating_add(1usize.saturating_add(self.preservation_controls.len())),
            );
        });
        if !self
            .leading_negative_ready_at
            .is_some_and(|ready_at| ready_at <= before)
        {
            return false;
        }
        let prefix = &self.value[..before];
        let preservation_controls = self
            .preservation_controls
            .iter()
            .filter(|(control, first_end)| {
                first_end.is_some_and(|end| end <= before)
                    || prefix_has_bounded_control_suffix(prefix, control)
            })
            .count();
        (1usize.saturating_add(preservation_controls)) % 2 == 1
    }
}

pub(super) fn prefix_has_bounded_control_suffix(prefix: &str, control: &str) -> bool {
    let prefix = prefix.trim_end();
    let Some(start) = prefix.len().checked_sub(control.len()) else {
        return false;
    };
    prefix[start..] == *control
        && prefix[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !word_continuation(character))
}

pub(super) fn secret_action_polarities(value: &str) -> Vec<(usize, bool)> {
    let mut polarities = vec![None; value.len().saturating_add(1)];
    for marker in SECRET_ACTIONS {
        #[cfg(test)]
        SECRET_ACTION_POLARITY_WORK.with(|work| {
            work.set(work.get().saturating_add(value.len().saturating_add(1)));
        });
        for (start, matched) in value.match_indices(marker) {
            let end = start.saturating_add(matched.len());
            if !marker_has_boundaries(value, start, end) {
                continue;
            }
            let polarity = !marker_is_negated(value, start, end);
            polarities[start] = Some(polarity);
            #[cfg(test)]
            SECRET_ACTION_POLARITY_WORK.with(|work| {
                work.set(work.get().saturating_add(1));
            });
        }
    }
    #[cfg(test)]
    SECRET_ACTION_POLARITY_WORK.with(|work| {
        work.set(work.get().saturating_add(polarities.len()));
    });
    polarities
        .into_iter()
        .enumerate()
        .filter_map(|(start, polarity)| polarity.map(|polarity| (start, polarity)))
        .collect()
}

pub(super) fn maximal_secret_target_occurrences(value: &str) -> Vec<(usize, usize)> {
    let mut furthest_end_at_start = vec![None; value.len().saturating_add(1)];
    for marker in SECRET_TARGETS {
        #[cfg(test)]
        MAXIMAL_SECRET_TARGET_WORK.with(|work| {
            work.set(work.get().saturating_add(value.len().saturating_add(1)));
        });
        for (start, matched) in value.match_indices(marker) {
            let end = start.saturating_add(matched.len());
            if !marker_has_boundaries(value, start, end) {
                continue;
            }
            let slot = &mut furthest_end_at_start[start];
            *slot = Some(slot.map_or(end, |current: usize| current.max(end)));
            #[cfg(test)]
            MAXIMAL_SECRET_TARGET_WORK.with(|work| {
                work.set(work.get().saturating_add(1));
            });
        }
    }
    let mut maximal = Vec::new();
    let mut max_end = 0usize;
    for (start, end) in furthest_end_at_start.into_iter().enumerate() {
        #[cfg(test)]
        MAXIMAL_SECRET_TARGET_WORK.with(|work| work.set(work.get().saturating_add(1)));
        let Some(end) = end else {
            continue;
        };
        if end <= max_end {
            continue;
        }
        max_end = end;
        maximal.push((start, end));
    }
    maximal
}

pub(super) fn has_unsafe_secret_target(value: &str) -> bool {
    maximal_secret_target_occurrences(value)
        .into_iter()
        .any(|(start, end)| !secret_target_is_locally_safe(value, start, end))
}

pub(in super::super) fn secret_target_is_locally_safe(
    value: &str,
    start: usize,
    end: usize,
) -> bool {
    if secret_target_has_value_reopener(&value[end..]) {
        return false;
    }
    let preceding = value[..start].split_whitespace().next_back();
    if preceding.is_some_and(|word| {
        matches!(
            word,
            "masked"
                | "redacted"
                | "replaced"
                | "substituted"
                | "가린"
                | "가려진"
                | "마스킹된"
                | "대체된"
                | "치환된"
                | "숨긴"
                | "숨겨진"
        )
    }) {
        return true;
    }
    if secret_target_is_metadata(value, start, end) {
        return true;
    }
    let suffix = value[end..].trim_start();
    [
        "is masked",
        "is redacted",
        "is replaced",
        "is substituted",
        "remains masked",
        "remains redacted",
    ]
    .iter()
    .any(|predicate| {
        suffix.strip_prefix(predicate).is_some_and(|remainder| {
            remainder
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

pub(super) fn secret_target_is_metadata(value: &str, start: usize, end: usize) -> bool {
    if secret_target_has_value_reopener(&value[end..]) {
        return false;
    }
    let prefix = value[..start].trim_end();
    if ["number of", "count of"]
        .iter()
        .any(|carrier| prefix.ends_with(carrier))
    {
        return true;
    }
    let prefix_words = prefix.split_whitespace().collect::<Vec<_>>();
    if prefix_words.len() >= 4
        && prefix_words[prefix_words.len().saturating_sub(4)..]
            .iter()
            .copied()
            .eq(["four", "characters", "of", "an"])
        && prefix_words
            .get(prefix_words.len().saturating_sub(5))
            .is_some_and(|word| *word == "last")
    {
        return true;
    }
    let suffix = value[end..].trim_start();
    [
        "configuration status",
        "expiry date",
        "expiration date",
        "fingerprint",
        "format",
        "health",
        "identifier",
        "is active",
        "is configured",
        "metadata",
        "policy",
        "requirements",
        "rotation status",
        "status",
        "usage count",
        "usage counts",
        "usage metric",
        "usage metrics",
    ]
    .iter()
    .any(|role| {
        suffix.strip_prefix(role).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

pub(super) fn secret_target_has_value_reopener(suffix: &str) -> bool {
    let suffix = suffix.trim_start();
    if bounded_marker_occurrences(
        suffix,
        &[
            "and actual value",
            "and content",
            "and its actual value",
            "and its raw value",
            "and its value",
            "and raw value",
            "and secret content",
            "and their values",
            "and value",
            "together with its actual value",
            "together with its raw value",
            "together with its value",
            "with its actual value",
            "with its raw value",
            "with its value",
        ],
    )
    .next()
    .is_some()
    {
        return true;
    }
    bounded_marker_occurrences(suffix, SECRET_ACTIONS)
        .any(|(_, end)| starts_with_secret_value_reference(suffix[end..].trim_start()))
}

pub(super) fn starts_with_secret_value_reference(value: &str) -> bool {
    [
        "actual value",
        "content",
        "its actual value",
        "its raw value",
        "its value",
        "raw value",
        "secret content",
        "the value",
        "their values",
        "value",
        "values",
    ]
    .iter()
    .any(|reference| {
        value.strip_prefix(reference).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

pub(in super::super) fn starts_with_secret_target_object(value: &str) -> bool {
    maximal_secret_target_occurrences(value)
        .into_iter()
        .any(|(start, _)| {
            value[..start].split_whitespace().all(|word| {
                matches!(
                    word,
                    "a" | "all"
                        | "an"
                        | "any"
                        | "each"
                        | "every"
                        | "masked"
                        | "raw"
                        | "real"
                        | "redacted"
                        | "the"
                        | "unmasked"
                        | "unredacted"
                )
            })
        })
}
