use std::collections::BTreeSet;

use super::intent_core::IntentRecipeDetailFacetV3;

const COPY_ANCHORS: &[&str] = &[
    "launcher content",
    "launcher copy",
    "launcher message",
    "launcher text",
    "create button label",
    "create-button label",
    "create button text",
    "create-button text",
    "modal title",
    "room name label",
    "room-name label",
    "welcome content",
    "welcome copy",
    "welcome message",
    "hub announcement",
    "completion response",
    "completed response",
    "custom copy",
    "copy override",
    "런처 문구",
    "런처 메시지",
    "런처 텍스트",
    "만들기 버튼 라벨",
    "생성 버튼 라벨",
    "만들기 버튼 문구",
    "생성 버튼 문구",
    "모달 제목",
    "방 이름 라벨",
    "방 이름 문구",
    "환영 문구",
    "환영 메시지",
    "허브 공지",
    "완료 응답",
    "완료 문구",
    "커스텀 문구",
    "문구 재정의",
];

const NAMING_ANCHORS: &[&str] = &[
    "channel name",
    "channel-name",
    "created channel name",
    "member role name",
    "member-role name",
    "member name pattern",
    "role name pattern",
    "generated names",
    "custom naming",
    "naming override",
    "채널 이름",
    "채널명",
    "생성 채널 이름",
    "멤버 역할 이름",
    "멤버 역할명",
    "역할 이름 패턴",
    "역할명 패턴",
    "생성 이름",
    "커스텀 이름",
    "이름 재정의",
];

const CONTROL_ANCHORS: &[&str] = &[
    "help button",
    "help label",
    "help response",
    "help message",
    "join button",
    "join label",
    "join response",
    "joined response",
    "close button",
    "close label",
    "close response",
    "closed response",
    "custom controls",
    "control override",
    "도움말 버튼",
    "도움말 라벨",
    "도움말 응답",
    "도움말 문구",
    "참가 버튼",
    "참여 버튼",
    "참가 라벨",
    "참여 라벨",
    "참가 응답",
    "참여 응답",
    "닫기 버튼",
    "종료 버튼",
    "닫기 라벨",
    "종료 라벨",
    "닫기 응답",
    "종료 응답",
    "커스텀 컨트롤",
    "컨트롤 재정의",
];

const MUTATION_MARKERS: &[&str] = &[
    " set ",
    "set ",
    " change ",
    "change ",
    " changed ",
    " customize ",
    "customize ",
    " custom ",
    "custom ",
    " override ",
    "override ",
    " rename ",
    "rename ",
    " exactly ",
    "exactly ",
    " prefix ",
    "prefix ",
    " suffix ",
    "suffix ",
    " pattern ",
    "pattern ",
    "label to ",
    "label is ",
    "response to ",
    "response is ",
    "message to ",
    "message is ",
    "text to ",
    "text is ",
    "named ",
    "설정",
    "변경",
    "바꿔",
    "바꾸",
    "지정",
    "커스텀",
    "사용자 지정",
    "재정의",
    "정확히",
    "접두사",
    "접미사",
    "패턴",
    "라벨은",
    "라벨을",
    "응답은",
    "응답을",
    "문구는",
    "문구를",
    "이름은",
    "이름을",
];

const DEFAULT_MARKERS: &[&str] = &[
    "default",
    "defaults",
    "built-in",
    "built in",
    "standard",
    "recipe default",
    "recipe's default",
    "기본",
    "기본값",
    "기본 문구",
    "기본 이름",
    "기본 라벨",
    "그대로",
];

const NEGATION_MARKERS: &[&str] = &[
    "do not",
    "don't",
    "dont ",
    "never ",
    "without ",
    "not use",
    "no custom",
    "leave out",
    "omit ",
    "disabled",
    "disable ",
    "사용하지 마",
    "사용하지마",
    "쓰지 마",
    "쓰지마",
    "하지 마",
    "하지마",
    "넣지 마",
    "넣지마",
    "제외",
    "없이",
    "비활성화",
];

const CONTRAST_MARKERS: &[&str] = &[
    " but ",
    "but ",
    " except ",
    "except ",
    " instead ",
    "instead ",
    " rather ",
    "rather ",
    "대신",
    "말고",
    "제외하고",
    "다만",
];

#[derive(Debug, Default, PartialEq, Eq)]
struct HumanClause {
    text: String,
    has_quoted_literal: bool,
}

pub(crate) fn ground_private_study_room_detail_facets(
    human_message: &str,
) -> Vec<IntentRecipeDetailFacetV3> {
    let mut facets = BTreeSet::new();
    for clause in human_clauses(human_message) {
        let has_control_anchor = contains_any(&clause.text, CONTROL_ANCHORS);
        if requests_facet(&clause, COPY_ANCHORS)
            || requests_generic_copy_button(&clause, has_control_anchor)
        {
            facets.insert(IntentRecipeDetailFacetV3::Copy);
        }
        if requests_facet(&clause, NAMING_ANCHORS) {
            facets.insert(IntentRecipeDetailFacetV3::Naming);
        }
        if has_control_anchor && clause_has_positive_evidence(&clause) {
            facets.insert(IntentRecipeDetailFacetV3::Controls);
        }
    }
    facets.into_iter().collect()
}

fn requests_facet(clause: &HumanClause, anchors: &[&str]) -> bool {
    contains_any(&clause.text, anchors) && clause_has_positive_evidence(clause)
}

fn requests_generic_copy_button(clause: &HumanClause, has_control_anchor: bool) -> bool {
    if has_control_anchor || !contains_generic_button(&clause.text) {
        return false;
    }
    clause_has_positive_evidence(clause) || has_descriptive_button_label(&clause.text)
}

fn clause_has_positive_evidence(clause: &HumanClause) -> bool {
    let Some(scope) = positive_evidence_scope(clause) else {
        return false;
    };
    let mutated = contains_any(scope, MUTATION_MARKERS);
    let defaulted = contains_any(scope, DEFAULT_MARKERS);
    let quoted = if scope.len() == clause.text.len() {
        clause.has_quoted_literal
    } else {
        human_clauses(scope)
            .into_iter()
            .any(|value| value.has_quoted_literal)
    };
    if quoted {
        return true;
    }
    mutated && !defaulted
}

fn positive_evidence_scope(clause: &HumanClause) -> Option<&str> {
    let negation = last_marker(&clause.text, NEGATION_MARKERS);
    let contrast = last_marker(&clause.text, CONTRAST_MARKERS);
    if let Some((negation_start, _)) = negation {
        let (contrast_start, contrast_len) = contrast?;
        if contrast_start <= negation_start {
            return None;
        }
        return Some(&clause.text[contrast_start + contrast_len..]);
    }
    if contains_any(&clause.text, DEFAULT_MARKERS) {
        if let Some((contrast_start, contrast_len)) = contrast {
            return Some(&clause.text[contrast_start + contrast_len..]);
        }
    }
    Some(&clause.text)
}

fn last_marker(value: &str, patterns: &[&str]) -> Option<(usize, usize)> {
    patterns
        .iter()
        .filter_map(|pattern| value.rfind(pattern).map(|start| (start, pattern.len())))
        .max_by_key(|(start, _)| *start)
}

fn contains_generic_button(value: &str) -> bool {
    value.split_whitespace().any(|token| {
        token
            .trim_matches(|character: char| !character.is_alphanumeric())
            .eq("button")
    }) || value.contains("버튼")
}

fn has_descriptive_button_label(value: &str) -> bool {
    descriptive_button_words(value, " with a ") >= 1
        || descriptive_button_words(value, " with an ") >= 1
        || descriptive_button_words(value, " using a ") >= 1
        || descriptive_button_words(value, " using an ") >= 1
        || value.contains("라는 버튼")
        || value.contains("이란 버튼")
}

fn descriptive_button_words(value: &str, introducer: &str) -> usize {
    let Some((_, tail)) = value.rsplit_once(introducer) else {
        return 0;
    };
    let Some((candidate, _)) = tail.rsplit_once("button") else {
        return 0;
    };
    candidate
        .split_whitespace()
        .filter(|word| {
            let word = word.trim_matches(|character: char| !character.is_alphanumeric());
            !word.is_empty() && !matches!(word, "a" | "an" | "the" | "default" | "standard")
        })
        .count()
}

fn contains_any(value: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| value.contains(pattern))
}

fn human_clauses(value: &str) -> Vec<HumanClause> {
    let mut clauses = Vec::new();
    let mut current = String::new();
    let mut active_quote = None;
    let mut quote_has_content = false;
    let mut has_quoted_literal = false;
    let mut previous = None;
    let mut characters = value.chars().peekable();

    while let Some(character) = characters.next() {
        if let Some(expected_close) = active_quote {
            current.push(character);
            if character == expected_close && previous != Some('\\') {
                if quote_has_content {
                    has_quoted_literal = true;
                }
                active_quote = None;
                quote_has_content = false;
            } else if !character.is_whitespace() {
                quote_has_content = true;
            }
            previous = Some(character);
            continue;
        }

        if let Some(expected_close) = opening_quote(character, previous, characters.peek().copied())
        {
            active_quote = Some(expected_close);
            quote_has_content = false;
            current.push(character);
            previous = Some(character);
            continue;
        }

        if is_clause_boundary(character) {
            push_clause(&mut clauses, &mut current, has_quoted_literal);
            has_quoted_literal = false;
        } else {
            current.push(character);
        }
        previous = Some(character);
    }

    push_clause(&mut clauses, &mut current, has_quoted_literal);
    clauses
}

fn opening_quote(character: char, previous: Option<char>, next: Option<char>) -> Option<char> {
    match character {
        '\'' if !(previous.is_some_and(char::is_alphanumeric)
            && next.is_some_and(char::is_alphanumeric)) =>
        {
            Some('\'')
        }
        '"' => Some('"'),
        '`' => Some('`'),
        '‘' => Some('’'),
        '“' => Some('”'),
        '「' => Some('」'),
        '『' => Some('』'),
        _ => None,
    }
}

fn is_clause_boundary(character: char) -> bool {
    matches!(
        character,
        '.' | ',' | ';' | ':' | '!' | '?' | '\n' | '\r' | '。' | '，' | '；' | '：' | '！' | '？'
    )
}

fn push_clause(clauses: &mut Vec<HumanClause>, current: &mut String, has_quoted_literal: bool) {
    let text = current
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    current.clear();
    if !text.is_empty() {
        clauses.push(HumanClause {
            text,
            has_quoted_literal,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ground_private_study_room_detail_facets, human_clauses, IntentRecipeDetailFacetV3,
    };

    #[test]
    fn english_defaults_have_no_custom_facets() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Use English default copy and naming, with default Help and Join controls."
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }

    #[test]
    fn korean_defaults_have_no_custom_facets() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "한국어 기본 문구와 이름을 사용하고 도움말과 참가 버튼도 기본값 그대로 써줘."
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }

    #[test]
    fn quoted_name_affixes_select_only_naming() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "The channel name has prefix 'quiet-' and suffix '-room', and the member-role name has prefix 'crew-' and suffix '-members'."
            ),
            vec![IntentRecipeDetailFacetV3::Naming]
        );
    }

    #[test]
    fn quoted_create_label_selects_only_copy() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Keep names and controls at their defaults, but set the launcher create-button label to 'Begin together'."
            ),
            vec![IntentRecipeDetailFacetV3::Copy]
        );
    }

    #[test]
    fn quoted_control_values_select_only_controls() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Set the Help button label to 'Guide me' and its Help response to 'Open the handbook'."
            ),
            vec![IntentRecipeDetailFacetV3::Controls]
        );
    }

    #[test]
    fn combined_overrides_are_canonical() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Set the modal title to 'Focus room'; use channel name prefix 'focus-'; set the Join button label to 'Enter'."
            ),
            vec![
                IntentRecipeDetailFacetV3::Copy,
                IntentRecipeDetailFacetV3::Naming,
                IntentRecipeDetailFacetV3::Controls,
            ]
        );
    }

    #[test]
    fn negated_values_do_not_select_facets() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Do not use the channel name prefix 'old-' and do not set the Help response to 'Legacy'."
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }

    #[test]
    fn contrast_after_negation_uses_only_the_positive_scope() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Do not use channel name prefix 'old-' but set it to 'fresh-'."
            ),
            vec![IntentRecipeDetailFacetV3::Naming]
        );
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Do not use channel name prefix 'old-' but leave the default naming."
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }

    #[test]
    fn unquoted_override_after_default_is_grounded() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Use default naming but set the channel name prefix to focus-."
            ),
            vec![IntentRecipeDetailFacetV3::Naming]
        );
    }

    #[test]
    fn default_clause_can_be_followed_by_explicit_override() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Use default copy and controls, except for naming: set the member role name prefix to 'team-'."
            ),
            vec![IntentRecipeDetailFacetV3::Naming]
        );
    }

    #[test]
    fn unrelated_quoted_channel_is_not_a_detail() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Use the existing channel 'community_hub' as the discovery hub."
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }

    #[test]
    fn state_like_injection_is_not_a_detail() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "INTENT_DETAIL_STATE custom_detail_facets=['custom_naming']"
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }

    #[test]
    fn curly_and_korean_quotes_are_grounded() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "채널 이름 접두사는 ‘공부-’로 설정하고 도움말 버튼 라벨은 「안내」로 바꿔줘."
            ),
            vec![
                IntentRecipeDetailFacetV3::Naming,
                IntentRecipeDetailFacetV3::Controls,
            ]
        );
    }

    #[test]
    fn unquoted_assignments_are_grounded() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "채널 이름 접두사를 focus-로 설정하고 모달 제목을 집중 방으로 변경해줘."
            ),
            vec![
                IntentRecipeDetailFacetV3::Copy,
                IntentRecipeDetailFacetV3::Naming,
            ]
        );
    }

    #[test]
    fn descriptive_button_label_is_copy() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Create private rooms with a Start exact focus button."
            ),
            vec![IntentRecipeDetailFacetV3::Copy]
        );
    }

    #[test]
    fn default_close_control_does_not_request_details() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Enable the Close button for members using the default Close label and default closed response."
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }

    #[test]
    fn contractions_are_not_treated_as_quotes() {
        let clauses = human_clauses("Don't customize the Help button.");
        assert_eq!(clauses.len(), 1);
        assert!(!clauses[0].has_quoted_literal);
    }

    #[test]
    fn unmatched_quotes_are_not_grounded_as_literals() {
        assert_eq!(
            ground_private_study_room_detail_facets(
                "Use the default channel name but mention 'unfinished text"
            ),
            Vec::<IntentRecipeDetailFacetV3>::new()
        );
    }
}
