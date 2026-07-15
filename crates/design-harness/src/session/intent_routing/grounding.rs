#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectionOccurrence {
    option: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectionToken {
    value: String,
    start: usize,
    end: usize,
}

pub(super) fn unambiguously_selects_option(
    human_message: &str,
    selected: &str,
    options: &[String],
) -> bool {
    deterministically_selected_option(human_message, options).as_deref() == Some(selected)
}

pub(super) fn deterministically_selected_option(
    human_message: &str,
    options: &[String],
) -> Option<String> {
    let message_tokens = normalized_selection_tokens(human_message);
    let display_occurrences = maximal_display_occurrences(
        options
            .iter()
            .flat_map(|option| display_occurrences(option, human_message, &message_tokens))
            .collect(),
    );
    let raw_occurrences = maximal_raw_occurrences(
        options
            .iter()
            .flat_map(|option| raw_occurrences(option, human_message))
            .collect(),
    )
    .into_iter()
    .filter(|raw| {
        !display_occurrences.iter().any(|display| {
            display.option != raw.option
                && display.start <= raw.start
                && display.end >= raw.end
                && (display.start < raw.start || display.end > raw.end)
        })
    })
    .collect::<Vec<_>>();
    let raw_options = raw_occurrences
        .iter()
        .map(|occurrence| occurrence.option.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if raw_options.len() > 1 {
        return None;
    }
    if let Some(raw_option) = raw_options.first() {
        let conflicting_display = display_occurrences.iter().any(|display| {
            display.option != *raw_option
                && !raw_occurrences.iter().any(|raw| {
                    raw.option == *raw_option
                        && raw.start == display.start
                        && raw.end == display.end
                })
        });
        if conflicting_display {
            return None;
        }
        if raw_occurrences
            .iter()
            .filter(|occurrence| occurrence.option == *raw_option)
            .any(|occurrence| selection_is_uncertain_or_contrasted(human_message, occurrence))
        {
            return None;
        }
        return Some((*raw_option).to_string());
    }
    let display_options = display_occurrences
        .iter()
        .map(|occurrence| occurrence.option.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if display_options.len() != 1 {
        return None;
    }
    let selected = *display_options.first().unwrap();
    if display_occurrences
        .iter()
        .filter(|occurrence| occurrence.option == selected)
        .any(|occurrence| selection_is_uncertain_or_contrasted(human_message, occurrence))
    {
        return None;
    }
    Some(selected.to_string())
}

fn raw_occurrences(option: &str, human_message: &str) -> Vec<SelectionOccurrence> {
    if option.is_empty() {
        return Vec::new();
    }
    human_message
        .match_indices(option)
        .filter_map(|(start, matched)| {
            let end = start + matched.len();
            let left = human_message[..start].chars().next_back();
            let right = human_message[end..].chars().next();
            (!left.is_some_and(is_binding_key_character)
                && !right.is_some_and(is_binding_key_character)
                && !has_left_key_extension(human_message, start)
                && !has_right_key_extension(human_message, end)
                && !has_separator_only_extension(option, left, right))
            .then(|| SelectionOccurrence {
                option: option.to_string(),
                start,
                end,
            })
        })
        .collect()
}

fn maximal_raw_occurrences(occurrences: Vec<SelectionOccurrence>) -> Vec<SelectionOccurrence> {
    occurrences
        .iter()
        .filter(|candidate| {
            !occurrences.iter().any(|other| {
                other.option != candidate.option
                    && other.start <= candidate.start
                    && other.end >= candidate.end
                    && (other.start < candidate.start || other.end > candidate.end)
            })
        })
        .cloned()
        .collect()
}

fn display_occurrences(
    option: &str,
    human_message: &str,
    message_tokens: &[SelectionToken],
) -> Vec<SelectionOccurrence> {
    let option_tokens = normalized_selection_tokens(option);
    if option_tokens.is_empty() || option_tokens.len() > message_tokens.len() {
        return Vec::new();
    }
    message_tokens
        .windows(option_tokens.len())
        .filter_map(|window| {
            let start = window.first().unwrap().start;
            let end = window.last().unwrap().end;
            (window
                .iter()
                .map(|token| token.value.as_str())
                .eq(option_tokens.iter().map(|token| token.value.as_str()))
                && !has_left_key_extension(human_message, start)
                && !has_right_key_extension(human_message, end))
            .then(|| SelectionOccurrence {
                option: option.to_string(),
                start,
                end,
            })
        })
        .collect()
}

fn maximal_display_occurrences(occurrences: Vec<SelectionOccurrence>) -> Vec<SelectionOccurrence> {
    occurrences
        .iter()
        .filter(|candidate| {
            !occurrences.iter().any(|other| {
                other.option != candidate.option
                    && other.start <= candidate.start
                    && other.end >= candidate.end
                    && (other.start < candidate.start || other.end > candidate.end)
            })
        })
        .cloned()
        .collect()
}

fn normalized_selection_tokens(value: &str) -> Vec<SelectionToken> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in value.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(token_start) = start.take() {
            push_selection_token(&mut tokens, value, token_start, index);
        }
    }
    if let Some(token_start) = start {
        push_selection_token(&mut tokens, value, token_start, value.len());
    }
    tokens
}

fn push_selection_token(tokens: &mut Vec<SelectionToken>, source: &str, start: usize, end: usize) {
    let value = source[start..end].to_lowercase();
    let value = strip_korean_selection_particle(&value).to_string();
    if !value.is_empty() {
        tokens.push(SelectionToken { value, start, end });
    }
}

fn strip_korean_selection_particle(value: &str) -> &str {
    ["으로", "은", "는", "이", "가", "을", "를", "로"]
        .into_iter()
        .find_map(|suffix| {
            value.strip_suffix(suffix).filter(|stem| {
                stem.chars()
                    .any(|character| character.is_ascii_alphanumeric())
            })
        })
        .unwrap_or(value)
}

fn is_binding_key_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn has_left_key_extension(value: &str, start: usize) -> bool {
    let mut characters = value[..start].chars().rev();
    characters.next().is_some_and(is_key_separator)
        && characters
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
}

fn has_right_key_extension(value: &str, end: usize) -> bool {
    let mut characters = value[end..].chars();
    characters.next().is_some_and(is_key_separator)
        && characters
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
}

fn is_key_separator(character: char) -> bool {
    matches!(character, '-' | '.' | ':' | '/')
}

fn has_separator_only_extension(option: &str, left: Option<char>, right: Option<char>) -> bool {
    if option.chars().any(|character| character.is_alphanumeric()) {
        return false;
    }
    let first = option.chars().next();
    let last = option.chars().next_back();
    left == first || right == last
}

fn selection_is_uncertain_or_contrasted(value: &str, occurrence: &SelectionOccurrence) -> bool {
    let clause = selection_clause(value, occurrence.start, occurrence.end);
    if clause.contains('?') || clause.contains('？') {
        return true;
    }
    let lowercase = clause.to_lowercase();
    if [
        "don't", "can't", "won't", "말고", "아니", "대신", "아마", "혹시", "제외", "또는", "혹은",
        "않",
    ]
    .into_iter()
    .any(|marker| lowercase.contains(marker))
    {
        return true;
    }
    let tokens = normalized_selection_tokens(&lowercase)
        .into_iter()
        .map(|token| token.value)
        .collect::<Vec<_>>();
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "not"
                | "never"
                | "cannot"
                | "cant"
                | "dont"
                | "wont"
                | "isnt"
                | "maybe"
                | "perhaps"
                | "possibly"
                | "probably"
                | "uncertain"
                | "unsure"
                | "or"
                | "but"
                | "versus"
                | "vs"
                | "except"
                | "instead"
                | "rather"
                | "안"
                | "못"
        )
    })
}

fn selection_clause(value: &str, start: usize, end: usize) -> &str {
    let clause_start = value[..start]
        .char_indices()
        .rev()
        .find(|(_, character)| is_clause_boundary(*character))
        .map(|(index, character)| index + character.len_utf8())
        .unwrap_or(0);
    let clause_end = value[end..]
        .char_indices()
        .find(|(_, character)| is_clause_boundary(*character))
        .map(|(index, _)| end + index)
        .unwrap_or(value.len());
    &value[clause_start..clause_end]
}

fn is_clause_boundary(character: char) -> bool {
    matches!(character, '.' | '!' | ';' | '\n' | '\r')
}

#[cfg(test)]
mod tests {
    use super::{deterministically_selected_option, unambiguously_selects_option};

    #[test]
    fn option_grounding_accepts_exact_and_display_forms_only_when_unambiguous() {
        let options = vec!["community_hub".to_string(), "general_chat".to_string()];
        assert!(unambiguously_selects_option(
            "Use community_hub",
            "community_hub",
            &options
        ));
        assert!(unambiguously_selects_option(
            "Use the Community Hub",
            "community_hub",
            &options
        ));
        assert!(!unambiguously_selects_option(
            "Use community hub or general chat",
            "community_hub",
            &options
        ));
        assert!(!unambiguously_selects_option(
            "Use that one",
            "community_hub",
            &options
        ));
        let ambiguous = vec!["community_hub".to_string(), "community-hub".to_string()];
        assert!(!unambiguously_selects_option(
            "Use community hub",
            "community_hub",
            &ambiguous
        ));
    }

    #[test]
    fn option_grounding_gives_exact_raw_keys_precedence() {
        let colliding = vec!["community_hub".to_string(), "community-hub".to_string()];
        assert_eq!(
            deterministically_selected_option("Please use community_hub.", &colliding),
            Some("community_hub".to_string())
        );
        assert_eq!(
            deterministically_selected_option("Please use community-hub.", &colliding),
            Some("community-hub".to_string())
        );
        assert_eq!(
            deterministically_selected_option("Please use Community Hub.", &colliding),
            None
        );
    }

    #[test]
    fn option_grounding_resolves_prefixes_and_separator_only_keys() {
        let prefixed = vec!["hub".to_string(), "hub_archive".to_string()];
        assert_eq!(
            deterministically_selected_option("Use hub.", &prefixed),
            Some("hub".to_string())
        );
        assert_eq!(
            deterministically_selected_option("Use hub_archive.", &prefixed),
            Some("hub_archive".to_string())
        );
        assert_eq!(
            deterministically_selected_option("Use Hub Archive.", &prefixed),
            Some("hub_archive".to_string())
        );

        let dotted = vec!["hub".to_string(), "hub.archive".to_string()];
        assert_eq!(
            deterministically_selected_option("Use hub.archive.", &dotted),
            Some("hub.archive".to_string())
        );
        assert_eq!(
            deterministically_selected_option("Use hub.unknown.", &dotted),
            None
        );

        let separators = vec!["--".to_string(), "---".to_string()];
        assert_eq!(
            deterministically_selected_option("Use --- please.", &separators),
            Some("---".to_string())
        );
        assert_eq!(
            deterministically_selected_option("Use ---- please.", &separators),
            None
        );
    }

    #[test]
    fn option_grounding_rejects_negation_contrast_uncertainty_and_questions() {
        let options = vec!["community_hub".to_string(), "general_chat".to_string()];
        for message in [
            "Do not use community_hub",
            "Not community_hub",
            "community_hub 말고",
            "Maybe community_hub",
            "community_hub instead",
            "community_hub?",
            "community_hub or general_chat",
            "community_hub but not general_chat",
            "community_hub 아니면 general_chat",
            "Probably community_hub",
        ] {
            assert_eq!(
                deterministically_selected_option(message, &options),
                None,
                "{message}"
            );
        }
        assert_eq!(
            deterministically_selected_option(
                "study_hub는 아직 선택 안 했어",
                &["study_hub".to_string()],
            ),
            None
        );
    }

    #[test]
    fn option_grounding_preserves_unambiguous_affirmative_wrappers() {
        let options = vec!["community_hub".to_string(), "general_chat".to_string()];
        for message in [
            "Please use the Community Hub.",
            "I choose community_hub.",
            "채널은 community_hub로 해줘",
            "Community Hub으로 설정해줘",
            "Use community_hub. Do not rename the button.",
            "channel: community_hub",
        ] {
            assert_eq!(
                deterministically_selected_option(message, &options),
                Some("community_hub".to_string()),
                "{message}"
            );
        }
    }
}
