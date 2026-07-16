use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_detail_grammar::GroundedDetailAssignment;
use super::intent_detail_syntax::{
    canonical_material_detail_expectations, canonical_material_detail_fields,
    grounded_detail_assignment_scope, supported_detail_facets, supported_detail_fragment,
    supported_detail_syntax, IntentRecipeDetailFieldV4,
};

fn fields(requirement: &str) -> Option<Vec<IntentRecipeDetailFieldV4>> {
    supported_detail_syntax(requirement)
        .map(|syntax| canonical_material_detail_fields(syntax.assignments()))
}

fn expectations(requirement: &str) -> Option<Vec<(IntentRecipeDetailFieldV4, String)>> {
    supported_detail_syntax(requirement).map(|syntax| {
        canonical_material_detail_expectations(syntax.assignments())
            .iter()
            .map(|expectation| (expectation.field(), expectation.literal().to_owned()))
            .collect()
    })
}

#[test]
fn exact_literal_expectations_preserve_supported_quoted_and_unquoted_values() {
    for (requirement, field, literal) in [
        (
            "modal title is 'Deep   Focus'",
            IntentRecipeDetailFieldV4::ModalTitle,
            "Deep   Focus",
        ),
        (
            "channel name prefix to focus-",
            IntentRecipeDetailFieldV4::ChannelNamePrefix,
            "focus-",
        ),
        (
            "모달 제목을 '집중 방'으로 변경해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "집중 방",
        ),
        (
            "모달 제목을 집중 방으로 변경해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "집중 방",
        ),
        (
            "채널 이름 접두사를 'focus-'로 설정",
            IntentRecipeDetailFieldV4::ChannelNamePrefix,
            "focus-",
        ),
        (
            "채널 이름 접두사를 focus-로 설정",
            IntentRecipeDetailFieldV4::ChannelNamePrefix,
            "focus-",
        ),
        (
            "모달 제목을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "Deep-Focus",
        ),
        (
            "모달 제목을 집중/방으로 설정해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "집중/방",
        ),
        (
            "모달 제목을 🎯(집중 방)으로 설정해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "🎯(집중 방)",
        ),
        (
            "모달 제목을 A   B로 설정해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "A   B",
        ),
        (
            "도움말 버튼 라벨을 안내!로 변경해줘",
            IntentRecipeDetailFieldV4::HelpLabel,
            "안내!",
        ),
        (
            "도움말 버튼 라벨을 안내?로 변경해줘",
            IntentRecipeDetailFieldV4::HelpLabel,
            "안내?",
        ),
        (
            "도움말 버튼 라벨을 안내!?🧭로 변경해줘",
            IntentRecipeDetailFieldV4::HelpLabel,
            "안내!?🧭",
        ),
        (
            "모달 제목을 v1.0로 설정해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "v1.0",
        ),
        (
            "모달 제목을 🧭로 설정해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
            "🧭",
        ),
        (
            "도움말 버튼 라벨을 !로 변경해줘",
            IntentRecipeDetailFieldV4::HelpLabel,
            "!",
        ),
        (
            "채널 이름 접두사를 🧭로 설정",
            IntentRecipeDetailFieldV4::ChannelNamePrefix,
            "🧭",
        ),
    ] {
        assert_eq!(
            expectations(requirement),
            Some(vec![(field, literal.to_owned())]),
            "{requirement}"
        );
    }
}

#[test]
fn korean_unquoted_symbol_only_values_require_raw_material() {
    for requirement in [
        "모달 제목을 로 설정해줘",
        "모달 제목을 으로 설정해줘",
        "채널 이름 접두사를 로 설정",
        "채널 이름 접두사를 으로 설정",
    ] {
        assert_eq!(expectations(requirement), None, "{requirement}");
    }
}

#[test]
fn korean_unquoted_direct_literal_spans_cover_every_supported_direct_slot() {
    for (requirement, field) in [
        (
            "런처 만들기 버튼 라벨을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::CreateButtonLabel,
        ),
        (
            "모달 제목을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::ModalTitle,
        ),
        (
            "도움말 버튼 라벨을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::HelpLabel,
        ),
        (
            "도움말 응답을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::HelpResponse,
        ),
        (
            "참가 버튼 라벨을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::JoinLabel,
        ),
        (
            "참가 응답을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::JoinedResponse,
        ),
        (
            "닫기 버튼 라벨을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::CloseLabel,
        ),
        (
            "닫기 응답을 Deep-Focus로 설정해줘",
            IntentRecipeDetailFieldV4::ClosedResponse,
        ),
    ] {
        assert_eq!(
            expectations(requirement),
            Some(vec![(field, "Deep-Focus".to_owned())]),
            "{requirement}"
        );
    }
}

#[test]
fn exact_literal_expectations_are_canonical_and_omit_explicit_empty_values() {
    let requirement = "closed response is 'Closed' and channel name prefix to Focus- and channel name uses an empty suffix and launcher content is 'Create now'";
    assert_eq!(
        expectations(requirement),
        Some(vec![
            (
                IntentRecipeDetailFieldV4::LauncherContent,
                "Create now".to_owned(),
            ),
            (
                IntentRecipeDetailFieldV4::ChannelNamePrefix,
                "Focus-".to_owned(),
            ),
            (
                IntentRecipeDetailFieldV4::ClosedResponse,
                "Closed".to_owned(),
            ),
        ])
    );
}

#[test]
fn every_serving_leaf_has_one_material_field_mapping() {
    for (requirement, expected) in [
        (
            "launcher content is 'Create a room'",
            IntentRecipeDetailFieldV4::LauncherContent,
        ),
        (
            "launcher create button label is 'Start'",
            IntentRecipeDetailFieldV4::CreateButtonLabel,
        ),
        (
            "modal title is 'New room'",
            IntentRecipeDetailFieldV4::ModalTitle,
        ),
        (
            "room name label is 'Room name'",
            IntentRecipeDetailFieldV4::RoomNameLabel,
        ),
        (
            "welcome content prefix is 'Welcome '",
            IntentRecipeDetailFieldV4::WelcomeContentPrefix,
        ),
        (
            "welcome content suffix is ' ready'",
            IntentRecipeDetailFieldV4::WelcomeContentSuffix,
        ),
        (
            "hub announcement prefix is 'Created '",
            IntentRecipeDetailFieldV4::HubAnnouncementPrefix,
        ),
        (
            "hub announcement suffix is ' now'",
            IntentRecipeDetailFieldV4::HubAnnouncementSuffix,
        ),
        (
            "completed response prefix is 'Ready '",
            IntentRecipeDetailFieldV4::CompletedResponsePrefix,
        ),
        (
            "completed response suffix is ' done'",
            IntentRecipeDetailFieldV4::CompletedResponseSuffix,
        ),
        (
            "channel name prefix is 'focus-'",
            IntentRecipeDetailFieldV4::ChannelNamePrefix,
        ),
        (
            "channel name suffix is '-room'",
            IntentRecipeDetailFieldV4::ChannelNameSuffix,
        ),
        (
            "member role name prefix is 'team-'",
            IntentRecipeDetailFieldV4::MemberRoleNamePrefix,
        ),
        (
            "member role name suffix is '-members'",
            IntentRecipeDetailFieldV4::MemberRoleNameSuffix,
        ),
        (
            "Help button label is 'Guide'",
            IntentRecipeDetailFieldV4::HelpLabel,
        ),
        (
            "Help response is 'Read this'",
            IntentRecipeDetailFieldV4::HelpResponse,
        ),
        (
            "Join button label is 'Enter'",
            IntentRecipeDetailFieldV4::JoinLabel,
        ),
        (
            "joined response is 'Joined'",
            IntentRecipeDetailFieldV4::JoinedResponse,
        ),
        (
            "Close button label is 'Finish'",
            IntentRecipeDetailFieldV4::CloseLabel,
        ),
        (
            "closed response is 'Closed'",
            IntentRecipeDetailFieldV4::ClosedResponse,
        ),
    ] {
        assert_eq!(fields(requirement), Some(vec![expected]), "{requirement}");
    }
}

#[test]
fn material_fields_are_canonical_and_serialize_as_serving_leaf_names() {
    let requirement = "closed response is 'Closed' and channel name suffix is '-room' and launcher content is 'Create' and Help button label is 'Guide'";
    let fields = fields(requirement).unwrap();
    assert_eq!(
        fields,
        vec![
            IntentRecipeDetailFieldV4::LauncherContent,
            IntentRecipeDetailFieldV4::ChannelNameSuffix,
            IntentRecipeDetailFieldV4::HelpLabel,
            IntentRecipeDetailFieldV4::ClosedResponse,
        ]
    );
    assert_eq!(
        serde_json::to_value(fields).unwrap(),
        serde_json::json!([
            "launcher_content",
            "channel_name_suffix",
            "help_label",
            "closed_response"
        ])
    );
    assert_eq!(
        IntentRecipeDetailFieldV4::CompletedResponseSuffix.as_str(),
        "completed_response_suffix"
    );
}

#[test]
fn explicit_empty_affixes_do_not_create_material_fields() {
    assert_eq!(
        fields("channel name prefix is 'focus-' and an empty suffix"),
        Some(vec![IntentRecipeDetailFieldV4::ChannelNamePrefix])
    );
    assert_eq!(
        fields("channel name uses an empty suffix"),
        Some(Vec::new())
    );
}

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
fn exact_adverbs_and_possessive_response_fragments_remain_closed() {
    assert_eq!(
        supported_detail_facets(
            "room Help button label is exactly 'Guide' and its ephemeral response is exactly 'Read the guide'"
        ),
        Some(vec![IntentRecipeDetailFacetV3::Controls])
    );
    for requirement in [
        "its ephemeral response is 'Read this first'",
        "its ephemeral response is exactly 'Read the guide'",
    ] {
        assert_eq!(supported_detail_facets(requirement), None);
        assert!(supported_detail_fragment(requirement));
    }
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

#[test]
fn grounded_masked_detail_scope_reuses_the_complete_closed_grammar() {
    for requirement in [
        "use english defaults except for exact overrides modal title is plus help button label is",
        "change the help button label to",
        "도움말 버튼 라벨을 로 변경해",
        "도움말 버튼 라벨을 으로 변경해",
    ] {
        assert_eq!(
            grounded_detail_assignment_scope(requirement),
            Some(GroundedDetailAssignment::Static),
            "static grounded detail rejected for {requirement}"
        );
    }
    for requirement in [
        "modal title changes when archived",
        "on weekends set the help button label to",
        "welcome content changes on weekends",
        "help response changes after a restart",
    ] {
        assert_eq!(
            grounded_detail_assignment_scope(requirement),
            Some(GroundedDetailAssignment::Unsupported),
            "unsupported grounded detail accepted for {requirement}"
        );
    }
    assert_eq!(grounded_detail_assignment_scope("copy and naming"), None);
    assert_eq!(
        grounded_detail_assignment_scope("archive the help button label in an audit log"),
        None
    );
}
