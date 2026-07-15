pub(super) fn opening_quote(
    character: char,
    previous: Option<char>,
    next: Option<char>,
) -> Option<char> {
    match character {
        '\'' if !is_inner_apostrophe(previous, next) => Some('\''),
        '"' => Some('"'),
        '`' => Some('`'),
        '‘' => Some('’'),
        '“' => Some('”'),
        '「' => Some('」'),
        '『' => Some('』'),
        '«' => Some('»'),
        '‹' => Some('›'),
        '《' => Some('》'),
        '〈' => Some('〉'),
        _ => None,
    }
}

pub(super) fn closes_quote(
    character: char,
    expected_close: char,
    previous: Option<char>,
    next: Option<char>,
) -> bool {
    character == expected_close
        && previous != Some('\\')
        && !(matches!(expected_close, '\'' | '’') && is_inner_apostrophe(previous, next))
}

pub(super) fn normalized_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_inner_apostrophe(previous: Option<char>, next: Option<char>) -> bool {
    previous.is_some_and(|character| character.is_ascii_alphanumeric())
        && next.is_some_and(|character| character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{closes_quote, opening_quote};

    #[test]
    fn apostrophes_inside_words_are_not_quote_boundaries() {
        assert_eq!(opening_quote('\'', Some('n'), Some('t')), None);
        assert!(!closes_quote('\'', '\'', Some('n'), Some('t')));
        assert!(closes_quote('\'', '\'', Some('c'), Some('.')));
    }
}
