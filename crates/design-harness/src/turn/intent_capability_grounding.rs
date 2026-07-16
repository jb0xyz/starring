use std::collections::BTreeSet;

use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

use super::intent_metalinguistic_scope::{
    is_design_interaction_directive, semantic_unit_delimiter, trim_terminal_semantic_delimiters,
};

const DETERMINERS: &[&str] = &["a", "an", "each", "every", "the"];

const PREDICATE_WORDS: &[&str] = &[
    "advance",
    "advances",
    "are",
    "approve",
    "approves",
    "archive",
    "archives",
    "assign",
    "assigns",
    "award",
    "awards",
    "calculate",
    "calculates",
    "can",
    "close",
    "closes",
    "could",
    "create",
    "creates",
    "decide",
    "decides",
    "did",
    "do",
    "does",
    "earn",
    "earns",
    "emit",
    "emits",
    "execute",
    "executes",
    "gain",
    "gains",
    "grant",
    "grants",
    "had",
    "has",
    "have",
    "is",
    "lose",
    "loses",
    "make",
    "makes",
    "may",
    "might",
    "move",
    "moves",
    "must",
    "open",
    "opens",
    "post",
    "posts",
    "preserve",
    "preserves",
    "receive",
    "receives",
    "record",
    "records",
    "remove",
    "removes",
    "require",
    "requires",
    "respond",
    "responds",
    "run",
    "runs",
    "send",
    "sends",
    "shall",
    "should",
    "sign",
    "signs",
    "start",
    "starts",
    "stop",
    "stops",
    "survive",
    "survives",
    "trigger",
    "triggers",
    "unlock",
    "unlocks",
    "update",
    "updates",
    "use",
    "uses",
    "was",
    "were",
    "will",
    "would",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CapabilityEvidenceGroundingError {
    Ambiguous,
    ExpandedTooLong,
    Ungrounded,
}

pub(super) fn ground_unmapped_capability_evidence(
    human: &str,
    candidates: Vec<String>,
    max_chars: usize,
) -> Result<Vec<String>, CapabilityEvidenceGroundingError> {
    let mut grounded = BTreeSet::new();
    for candidate in candidates {
        if !has_bounded_occurrence(human, &candidate) {
            return Err(CapabilityEvidenceGroundingError::Ungrounded);
        }
        if has_subject_predicate_shape(&candidate)
            && has_repairable_occurrence(human, &candidate)
            && unique_bounded_occurrence(human, &candidate).is_none()
        {
            return Err(CapabilityEvidenceGroundingError::Ambiguous);
        }
        if grounded_meta_instruction(human, &candidate) {
            continue;
        }
        let value = expanded_missing_determiner(human, &candidate).unwrap_or(candidate);
        if value.encode_utf16().count() > max_chars {
            return Err(CapabilityEvidenceGroundingError::ExpandedTooLong);
        }
        grounded.insert(value);
    }
    Ok(grounded.into_iter().collect())
}

fn grounded_meta_instruction(human: &str, candidate: &str) -> bool {
    if !candidate
        .trim_start()
        .chars()
        .next()
        .is_some_and(char::is_alphanumeric)
    {
        return false;
    }
    let Some((start, end)) = unique_bounded_occurrence(human, candidate) else {
        return false;
    };
    let semantic_candidate = trim_terminal_semantic_delimiters(candidate);
    let semantic_end = end.saturating_sub(candidate.len().saturating_sub(semantic_candidate.len()));
    let design_interaction = is_design_interaction_directive(semantic_candidate);
    let starts_owned_clause = if design_interaction {
        starts_standalone_clause(human, start)
            || starts_bounded_so_design_clause(human, start)
            || starts_design_conjunct(human, start)
    } else {
        starts_imperative_clause(human, start)
    };
    top_level_unquoted_at(human, start)
        && starts_owned_clause
        && ends_imperative_clause(human, semantic_end)
        && (english_meta_instruction(semantic_candidate)
            || korean_meta_instruction(semantic_candidate))
}

fn english_meta_instruction(value: &str) -> bool {
    if is_design_interaction_directive(value) {
        return true;
    }
    let lowercase = value.to_lowercase();
    let words = lowercase
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let mut words = words.as_slice();
    if words.first() == Some(&"please") {
        words = &words[1..];
    }
    if words
        .iter()
        .any(|word| matches!(*word, "and" | "but" | "then" | "when" | "where" | "while"))
    {
        return false;
    }
    let (action, object) = if words.len() >= 3
        && (words.starts_with(&["do", "not"]) || words.starts_with(&["don", "t"]))
    {
        (words[2], &words[3..])
    } else if words.len() >= 2 && words.first() == Some(&"never") {
        (words[1], &words[2..])
    } else if let Some(action) = words.first() {
        (*action, &words[1..])
    } else {
        return false;
    };
    meta_action_word(action) && exact_meta_object(action, object)
}

fn exact_meta_object(action: &str, words: &[&str]) -> bool {
    let (target, tail) = if words.first().is_some_and(|word| {
        matches!(
            *word,
            "instruction"
                | "instructions"
                | "request"
                | "requests"
                | "requirement"
                | "requirements"
        )
    }) {
        (words[0], &words[1..])
    } else if words.len() >= 2
        && matches!(
            words[0],
            "all" | "our" | "that" | "the" | "these" | "this" | "those" | "your"
        )
    {
        (words[1], &words[2..])
    } else {
        return false;
    };
    if !matches!(
        target,
        "instruction" | "instructions" | "request" | "requests" | "requirement" | "requirements"
    ) {
        return false;
    }
    tail.is_empty()
        || (matches!(
            action,
            "reduce"
                | "reduces"
                | "reducing"
                | "reduced"
                | "simplify"
                | "simplifies"
                | "simplifying"
                | "simplified"
        ) && tail.len() <= 4
            && matches!(tail.first(), Some(&"into") | Some(&"to")))
        || tail == ["as", "written"]
}

fn meta_action_word(word: &str) -> bool {
    matches!(
        word,
        "omit"
            | "omits"
            | "omitted"
            | "omitting"
            | "preserve"
            | "preserved"
            | "preserves"
            | "preserving"
            | "reduce"
            | "reduced"
            | "reduces"
            | "reducing"
            | "simplify"
            | "simplified"
            | "simplifies"
            | "simplifying"
            | "summarize"
            | "summarized"
            | "summarizes"
            | "summarizing"
            | "weaken"
            | "weakened"
            | "weakening"
            | "weakens"
    )
}

fn korean_meta_instruction(value: &str) -> bool {
    if ["그리고", "하며", "하면서", ",", ";"]
        .iter()
        .any(|separator| value.contains(separator))
    {
        return false;
    }
    let value = value.strip_prefix("제발 ").unwrap_or(value);
    let Some(action) = [
        "이 요청을 ",
        "이 요청사항을 ",
        "요구사항을 ",
        "이 요구사항을 ",
        "지시사항을 ",
        "이 지시사항을 ",
    ]
    .iter()
    .find_map(|target| value.strip_prefix(target)) else {
        return false;
    };
    [
        "보존해",
        "보존하세요",
        "보존해야",
        "유지해",
        "유지하세요",
        "유지해야",
        "약화하지 마",
        "약화하지마",
        "약화시키지 마",
        "약화시키지마",
        "줄이지 마",
        "줄이지마",
        "축소하지 마",
        "축소하지마",
        "누락하지 마",
        "누락하지마",
        "빼먹지 마",
        "빼먹지마",
        "생략하지 마",
        "생략하지마",
        "단순화하지 마",
        "단순화하지마",
    ]
    .contains(&action)
}

fn expanded_missing_determiner(human: &str, candidate: &str) -> Option<String> {
    if !has_subject_predicate_shape(candidate) {
        return None;
    }
    let (start, end) = unique_bounded_occurrence(human, candidate)?;
    let determiner_start = determiner_before(human, start)?;
    human.get(determiner_start..end).map(str::to_string)
}

fn has_repairable_occurrence(human: &str, candidate: &str) -> bool {
    human.match_indices(candidate).any(|(start, _)| {
        bounded_occurrence(human, start, start + candidate.len())
            && determiner_before(human, start).is_some()
    })
}

fn determiner_before(human: &str, start: usize) -> Option<usize> {
    let prefix = human.get(..start)?.strip_suffix(' ')?;
    let determiner_start = prefix
        .rfind(char::is_whitespace)
        .map_or(0, |index| index.saturating_add(1));
    let determiner = prefix.get(determiner_start..)?;
    DETERMINERS
        .iter()
        .any(|allowed| determiner.eq_ignore_ascii_case(allowed))
        .then_some(determiner_start)
}

fn has_subject_predicate_shape(value: &str) -> bool {
    let tokens = value.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 3 {
        return false;
    }
    let subject = lexical_token(tokens[0]);
    let predicates = tokens
        .iter()
        .skip(1)
        .take(2)
        .map(|token| lexical_token(token));
    !subject.is_empty()
        && !DETERMINERS
            .iter()
            .any(|determiner| subject.eq_ignore_ascii_case(determiner))
        && predicates.into_iter().any(|predicate| {
            PREDICATE_WORDS
                .iter()
                .any(|word| predicate.eq_ignore_ascii_case(word))
        })
}

fn lexical_token(value: &str) -> &str {
    value.trim_matches(|character: char| !character.is_alphanumeric())
}

fn has_bounded_occurrence(human: &str, candidate: &str) -> bool {
    !candidate.is_empty()
        && human
            .match_indices(candidate)
            .any(|(start, _)| bounded_occurrence(human, start, start + candidate.len()))
}

fn unique_bounded_occurrence(human: &str, candidate: &str) -> Option<(usize, usize)> {
    if candidate.is_empty() {
        return None;
    }
    let mut occurrence = None;
    for (start, _) in human.match_indices(candidate) {
        let end = start + candidate.len();
        if !bounded_occurrence(human, start, end) {
            continue;
        }
        if occurrence.is_some() {
            return None;
        }
        occurrence = Some((start, end));
    }
    occurrence
}

fn top_level_unquoted_at(human: &str, target_start: usize) -> bool {
    let mut closing_quote = None;
    let mut previous = None;
    for (index, character) in human.char_indices() {
        if index >= target_start {
            break;
        }
        let next = human
            .get(index.saturating_add(character.len_utf8())..)
            .and_then(|value| value.chars().next());
        if let Some(expected) = closing_quote {
            if character == expected
                && !is_escaped(human, index)
                && !internal_apostrophe(character, previous, next)
            {
                closing_quote = None;
            }
        } else if let Some(expected) = closing_quote_for(character) {
            if !is_escaped(human, index)
                && !(apostrophe(character) && previous.is_some_and(char::is_alphanumeric))
            {
                closing_quote = Some(expected);
            }
        }
        previous = Some(character);
    }
    closing_quote.is_none()
}

fn starts_imperative_clause(human: &str, start: usize) -> bool {
    let Some(prefix) = human.get(..start) else {
        return false;
    };
    let boundary_prefix = prefix.trim_end_matches(|character: char| {
        character.is_whitespace() && !matches!(character, '\n' | '\r')
    });
    if boundary_prefix
        .chars()
        .next_back()
        .is_some_and(semantic_unit_delimiter)
    {
        return true;
    }
    let prefix = prefix.trim_end();
    if prefix.is_empty() {
        return true;
    }
    prefix
        .split(|character: char| !character.is_alphanumeric())
        .rfind(|word| !word.is_empty())
        .is_some_and(|word| word.eq_ignore_ascii_case("and") || word == "그리고")
}

fn starts_bounded_so_clause(human: &str, start: usize) -> bool {
    let Some(prefix) = human.get(..start).map(str::trim_end) else {
        return false;
    };
    let Some(connector) = prefix.split_whitespace().next_back() else {
        return false;
    };
    if !connector.eq_ignore_ascii_case("so") {
        return false;
    }
    prefix
        .get(..prefix.len().saturating_sub(connector.len()))
        .map(str::trim_end)
        .and_then(|value| value.chars().next_back())
        .is_some_and(|character| matches!(character, ',' | '，'))
}

fn starts_standalone_clause(human: &str, start: usize) -> bool {
    let Some(prefix) = human.get(..start) else {
        return false;
    };
    let prefix = prefix.trim_end_matches(|character: char| {
        character.is_whitespace() && !matches!(character, '\n' | '\r')
    });
    prefix.is_empty()
        || prefix
            .chars()
            .next_back()
            .is_some_and(semantic_unit_delimiter)
}

fn starts_bounded_so_design_clause(human: &str, start: usize) -> bool {
    if !starts_bounded_so_clause(human, start) {
        return false;
    }
    let prefix = human.get(..start).unwrap_or_default();
    let context = prefix
        .rsplit_once(',')
        .map(|(context, _)| context)
        .unwrap_or_default()
        .to_lowercase();
    [
        "choice",
        "choices",
        "detail",
        "details",
        "everything",
        "information",
        "material",
        "requirement",
        "requirements",
        "선택",
        "요구사항",
        "정보",
    ]
    .iter()
    .any(|marker| context.contains(marker))
}

fn starts_design_conjunct(human: &str, start: usize) -> bool {
    let prefix = human.get(..start).unwrap_or_default().trim_end();
    let Some(context) = prefix.strip_suffix("and").map(str::trim_end) else {
        return false;
    };
    let context = context
        .rsplit(semantic_unit_delimiter)
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let design_request = ["build", "create", "design", "make", "prepare", "set up"]
        .iter()
        .any(|verb| context.starts_with(verb) || context.starts_with(&format!("please {verb}")))
        && [
            "automation",
            "bot",
            "design",
            "request",
            "system",
            "workflow",
        ]
        .iter()
        .any(|target| context.contains(target));
    let executable_scope = [" clicked", " sends ", " posts ", " responds ", "when "]
        .iter()
        .any(|marker| context.contains(marker));
    design_request && !executable_scope
}

fn ends_imperative_clause(human: &str, end: usize) -> bool {
    let Some(suffix) = human.get(end..) else {
        return false;
    };
    if suffix
        .chars()
        .take_while(|character| character.is_whitespace())
        .any(|character| matches!(character, '\n' | '\r'))
    {
        return true;
    }
    let suffix = suffix.trim_start();
    suffix.is_empty() || suffix.chars().next().is_some_and(semantic_unit_delimiter)
}

fn closing_quote_for(character: char) -> Option<char> {
    match character {
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        '\u{2018}' => Some('\u{2019}'),
        '\u{201c}' => Some('\u{201d}'),
        '\u{00ab}' => Some('\u{00bb}'),
        '\u{2039}' => Some('\u{203a}'),
        '\u{3008}' => Some('\u{3009}'),
        '\u{300a}' => Some('\u{300b}'),
        '\u{300c}' => Some('\u{300d}'),
        '\u{300e}' => Some('\u{300f}'),
        '\u{3010}' => Some('\u{3011}'),
        _ => None,
    }
}

fn internal_apostrophe(character: char, previous: Option<char>, next: Option<char>) -> bool {
    apostrophe(character)
        && previous.is_some_and(char::is_alphanumeric)
        && next.is_some_and(char::is_alphanumeric)
}

fn is_escaped(value: &str, index: usize) -> bool {
    let Some(prefix) = value.get(..index) else {
        return false;
    };
    prefix
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count()
        % 2
        == 1
}

fn bounded_occurrence(human: &str, start: usize, end: usize) -> bool {
    left_is_boundary(human, start) && right_is_boundary(human, end)
}

fn left_is_boundary(human: &str, start: usize) -> bool {
    let Some(prefix) = human.get(..start) else {
        return false;
    };
    let Some(character) = prefix.chars().next_back() else {
        return true;
    };
    if apostrophe(character) {
        let prior = prefix
            .get(..prefix.len().saturating_sub(character.len_utf8()))
            .and_then(|value| value.chars().next_back());
        return prior.is_none_or(|value| !word_continuation(value));
    }
    !word_continuation(character)
}

fn right_is_boundary(human: &str, end: usize) -> bool {
    let Some(suffix) = human.get(end..) else {
        return false;
    };
    let Some(character) = suffix.chars().next() else {
        return true;
    };
    if apostrophe(character) {
        let next = suffix
            .get(character.len_utf8()..)
            .and_then(|value| value.chars().next());
        return next.is_none_or(|value| !word_continuation(value));
    }
    !word_continuation(character)
}

fn apostrophe(character: char) -> bool {
    matches!(
        character,
        '\'' | '\u{02bb}'
            | '\u{02bc}'
            | '\u{055a}'
            | '\u{05f3}'
            | '\u{07f4}'
            | '\u{07f5}'
            | '\u{2018}'
            | '\u{2019}'
            | '\u{201b}'
            | '\u{2032}'
            | '\u{a78b}'
            | '\u{a78c}'
            | '\u{ff07}'
    )
}

fn word_continuation(character: char) -> bool {
    if character.is_whitespace() {
        return false;
    }
    matches!(
        character.general_category_group(),
        GeneralCategoryGroup::Letter
            | GeneralCategoryGroup::Mark
            | GeneralCategoryGroup::Number
            | GeneralCategoryGroup::Other
    ) || matches!(
        character.general_category(),
        GeneralCategory::ConnectorPunctuation | GeneralCategory::DashPunctuation
    )
}
