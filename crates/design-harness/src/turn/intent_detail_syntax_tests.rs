use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_detail_syntax::{supported_detail_facets, supported_detail_fragment};

#[test]
fn every_closed_detail_slot_maps_to_its_canonical_facet() {
    for (requirement, expected) in [
        (
            "launcher content is 'Create a room'",
            IntentRecipeDetailFacetV3::Copy,
        ),
        (
            "launcher create button label is 'Start'",
            IntentRecipeDetailFacetV3::Copy,
        ),
        ("modal title is 'New room'", IntentRecipeDetailFacetV3::Copy),
        (
            "room name label is 'Room name'",
            IntentRecipeDetailFacetV3::Copy,
        ),
        (
            "welcome content prefix is 'Welcome '",
            IntentRecipeDetailFacetV3::Copy,
        ),
        (
            "hub announcement prefix is 'Created '",
            IntentRecipeDetailFacetV3::Copy,
        ),
        (
            "completion response prefix is 'Ready '",
            IntentRecipeDetailFacetV3::Copy,
        ),
        (
            "channel name prefix is 'focus-'",
            IntentRecipeDetailFacetV3::Naming,
        ),
        (
            "member role name prefix is 'member-'",
            IntentRecipeDetailFacetV3::Naming,
        ),
        (
            "Help button label is 'Guide'",
            IntentRecipeDetailFacetV3::Controls,
        ),
        (
            "Help response is 'Read this'",
            IntentRecipeDetailFacetV3::Controls,
        ),
        (
            "Join button label is 'Enter'",
            IntentRecipeDetailFacetV3::Controls,
        ),
        (
            "joined response is 'Joined'",
            IntentRecipeDetailFacetV3::Controls,
        ),
        (
            "Close button label is 'Finish'",
            IntentRecipeDetailFacetV3::Controls,
        ),
        (
            "closed response is 'Closed'",
            IntentRecipeDetailFacetV3::Controls,
        ),
    ] {
        assert_eq!(
            supported_detail_facets(requirement),
            Some(vec![expected]),
            "unexpected facet for {requirement}"
        );
    }
}

#[test]
fn plain_literal_slots_accept_only_direct_values() {
    for slot in [
        "launcher content",
        "launcher create button label",
        "modal title",
        "room name label",
        "Help button label",
        "Help response",
        "Join button label",
        "joined response",
        "Close button label",
        "closed response",
    ] {
        let direct = format!("{slot} is 'Value'");
        let affix = format!("{slot} prefix is 'Value'");
        assert!(
            supported_detail_facets(&direct).is_some(),
            "direct value rejected for {slot}"
        );
        assert_eq!(
            supported_detail_facets(&affix),
            None,
            "affix value accepted for {slot}"
        );
    }
}

#[test]
fn pattern_slots_accept_only_affix_values() {
    for slot in [
        "welcome content",
        "hub announcement",
        "completed response",
        "channel name",
        "member role name",
    ] {
        let affix = format!("{slot} prefix is 'Value'");
        let direct = format!("{slot} is 'Value'");
        assert!(
            supported_detail_facets(&affix).is_some(),
            "affix value rejected for {slot}"
        );
        assert_eq!(
            supported_detail_facets(&direct),
            None,
            "direct value accepted for {slot}"
        );
    }
}

#[test]
fn ephemeral_response_is_only_a_concrete_control_continuation() {
    for control in [
        "Help button label is 'Guide'",
        "Join button label is 'Enter'",
        "Close button label is 'Finish'",
    ] {
        let requirement = format!("{control} and its ephemeral response is 'Private'");
        assert_eq!(
            supported_detail_facets(&requirement),
            Some(vec![IntentRecipeDetailFacetV3::Controls]),
            "ephemeral continuation rejected for {control}"
        );
    }

    for requirement in [
        "ephemeral response is 'Private'",
        "ephemeral message is 'Private'",
        "its ephemeral response is 'Private'",
        "modal title is 'Room' and its ephemeral response is 'Private'",
        "일회성 응답을 '비공개'로 설정",
        "에페메랄 응답을 '비공개'로 설정",
        "Help button label is 'Guide' and its ephemeral response is 'Private' and its response is 'Done'",
    ] {
        assert_eq!(
            supported_detail_facets(requirement),
            None,
            "standalone ephemeral response accepted for {requirement}"
        );
    }
}

#[test]
fn combined_closed_details_return_one_canonical_facet_set() {
    assert_eq!(
        supported_detail_facets(
            "modal title is 'Room' and channel name prefix is 'focus-' and Help label is 'Guide'"
        ),
        Some(vec![
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ])
    );
}

#[test]
fn empty_affixes_and_conflicting_assignments_fail_closed() {
    for requirement in [
        "channel name uses an empty prefix",
        "channel name uses an empty suffix and Help button label is 'Guide'",
        "Help button label is 'Guide' and Help button label is 'Assist'",
        "channel name prefix is 'focus-' and prefix is 'study-'",
    ] {
        assert_eq!(
            supported_detail_facets(requirement),
            None,
            "conflicting or empty-only assignment accepted for {requirement}"
        );
    }
    assert_eq!(
        supported_detail_facets("channel name prefix is 'focus-' and an empty suffix"),
        Some(vec![IntentRecipeDetailFacetV3::Naming])
    );
    assert_eq!(
        supported_detail_facets("channel name prefix is 'focus-' and suffix is '-room'"),
        Some(vec![IntentRecipeDetailFacetV3::Naming])
    );
}

#[test]
fn generic_ephemeral_text_is_only_a_requirement_fragment() {
    let requirement = "ephemeral response is 'Private'";
    assert_eq!(supported_detail_facets(requirement), None);
    assert!(supported_detail_fragment(requirement));
}

#[test]
fn unquoted_values_require_a_closed_assignment_tail() {
    for requirement in [
        "channel name prefix to focus-",
        "채널 이름 접두사를 focus-로 설정",
        "모달 제목을 집중 방으로 변경해줘",
    ] {
        assert!(
            supported_detail_facets(requirement).is_some(),
            "closed unquoted value rejected for {requirement}"
        );
    }
    for requirement in [
        "channel name prefix to winner- on approval",
        "Help response is disabled",
        "Help response is omitted",
        "Help response is without",
        "channel name suffix is default",
        "modal title is standard",
        "modal title is unchanged",
        "modal title is none",
        "modal title is off",
        "modal title is forbidden",
        "채널 이름 접두사는 승인-로 설정하면",
        "채널명 x로 설정 금지",
        "모달 제목을 기본으로 설정해줘",
        "모달 제목을 기본값으로 설정해줘",
    ] {
        assert_eq!(
            supported_detail_facets(requirement),
            None,
            "unknown tail accepted for {requirement}"
        );
    }
}

#[test]
fn korean_compound_assignments_keep_their_typed_slots() {
    assert_eq!(
        supported_detail_facets(
            "채널 이름 접두사는 ‘공부-’로 설정하고 도움말 버튼 라벨은 「안내」로 바꿔줘"
        ),
        Some(vec![
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ])
    );
}

#[test]
fn contractions_inside_single_quoted_values_remain_one_literal() {
    for requirement in [
        "Help response is 'Don't panic'",
        "joined response is 'You're in'",
        "Help response is ‘Don’t panic’",
    ] {
        assert_eq!(
            supported_detail_facets(requirement),
            Some(vec![IntentRecipeDetailFacetV3::Controls])
        );
    }

    assert_eq!(
        supported_detail_facets("Help response is 'disabled'"),
        Some(vec![IntentRecipeDetailFacetV3::Controls])
    );
}
