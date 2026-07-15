pub(super) fn strip_repeated_prefixes<'a>(mut value: &'a str, prefixes: &[&str]) -> &'a str {
    loop {
        let Some(tail) = prefixes
            .iter()
            .find_map(|prefix| value.strip_prefix(prefix))
        else {
            return value;
        };
        value = tail;
    }
}

pub(super) fn ends_metalinguistic_copy(unit: &str) -> bool {
    matches!(
        unit,
        "end of example"
            | "end of payload"
            | "end of prompt"
            | "붙여넣기 끝"
            | "예시 끝"
            | "프롬프트 끝"
    )
}

pub(super) fn analyzes_metalinguistic_copy(unit: &str) -> bool {
    matches!(
        unit,
        "analyze the payload"
            | "analyze this payload"
            | "explain what the payload does"
            | "explain what this payload does"
            | "이 페이로드를 분석해"
            | "이 페이로드가 무엇을 하는지 설명해"
            | "페이로드를 분석해"
    )
}

pub(super) fn first_ascii_word_index(value: &str, expected: &str) -> Option<usize> {
    value.match_indices(expected).find_map(|(start, _)| {
        let end = start.saturating_add(expected.len());
        (value
            .get(..start)
            .and_then(|prefix| prefix.chars().next_back())
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
            && value
                .get(end..)
                .and_then(|suffix| suffix.chars().next())
                .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_'))
        .then_some(start)
    })
}

pub(super) fn last_ascii_word_index_before(
    value: &str,
    expected: &str,
    boundary: usize,
) -> Option<usize> {
    value
        .match_indices(expected)
        .filter_map(|(start, _)| {
            let end = start.saturating_add(expected.len());
            (start < boundary
                && value
                    .get(..start)
                    .and_then(|prefix| prefix.chars().next_back())
                    .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
                && value
                    .get(end..)
                    .and_then(|suffix| suffix.chars().next())
                    .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_'))
            .then_some(start)
        })
        .max()
}
