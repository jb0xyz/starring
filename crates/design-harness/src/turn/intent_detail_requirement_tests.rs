use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_detail_requirement::analyze_private_study_room_details;
use super::intent_detail_syntax::IntentRecipeDetailFieldV4;

fn facets(human: &str) -> Vec<IntentRecipeDetailFacetV3> {
    analyze_private_study_room_details(human).facets().to_vec()
}

fn fields(human: &str) -> Vec<IntentRecipeDetailFieldV4> {
    analyze_private_study_room_details(human).fields().to_vec()
}

fn expectations(human: &str) -> Vec<(IntentRecipeDetailFieldV4, String)> {
    analyze_private_study_room_details(human)
        .expectations()
        .iter()
        .map(|expectation| (expectation.field(), expectation.literal().to_owned()))
        .collect()
}

#[test]
fn analysis_exposes_canonical_field_literal_expectations_as_its_field_source() {
    let human = "Exact overrides: closed response is 'Closed'; channel name prefix to Focus-; modal title is 'Deep   Focus'; channel name uses an empty suffix.";
    let analysis = analyze_private_study_room_details(human);
    let expected = vec![
        (
            IntentRecipeDetailFieldV4::ModalTitle,
            "Deep   Focus".to_owned(),
        ),
        (
            IntentRecipeDetailFieldV4::ChannelNamePrefix,
            "Focus-".to_owned(),
        ),
        (
            IntentRecipeDetailFieldV4::ClosedResponse,
            "Closed".to_owned(),
        ),
    ];
    assert_eq!(
        analysis
            .expectations()
            .iter()
            .map(|expectation| (expectation.field(), expectation.literal().to_owned()))
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        analysis.fields(),
        expected.iter().map(|(field, _)| *field).collect::<Vec<_>>()
    );
    for (_, literal) in expected {
        assert!(human.contains(&literal), "missing {literal}");
    }
}

#[test]
fn analysis_preserves_korean_quoted_and_unquoted_exact_literals() {
    for (human, expected) in [
        (
            "모달 제목을 '집중 방'으로 변경해줘.",
            vec![(IntentRecipeDetailFieldV4::ModalTitle, "집중 방".to_owned())],
        ),
        (
            "모달 제목을 집중 방으로 변경해줘.",
            vec![(IntentRecipeDetailFieldV4::ModalTitle, "집중 방".to_owned())],
        ),
        (
            "채널 이름 접두사를 'focus-'로 설정.",
            vec![(
                IntentRecipeDetailFieldV4::ChannelNamePrefix,
                "focus-".to_owned(),
            )],
        ),
        (
            "채널 이름 접두사를 focus-로 설정하고 모달 제목을 집중 방으로 변경해줘.",
            vec![
                (IntentRecipeDetailFieldV4::ModalTitle, "집중 방".to_owned()),
                (
                    IntentRecipeDetailFieldV4::ChannelNamePrefix,
                    "focus-".to_owned(),
                ),
            ],
        ),
    ] {
        assert_eq!(expectations(human), expected, "{human}");
    }
}

#[test]
fn analysis_preserves_particle_bound_punctuation_and_rejects_final_questions() {
    for (human, field, literal) in [
        (
            "도움말 버튼 라벨을 안내!로 변경해줘.",
            IntentRecipeDetailFieldV4::HelpLabel,
            "안내!",
        ),
        (
            "도움말 버튼 라벨을 안내?로 변경해줘.",
            IntentRecipeDetailFieldV4::HelpLabel,
            "안내?",
        ),
        (
            "도움말 버튼 라벨을 안내!?🧭로 변경해줘.",
            IntentRecipeDetailFieldV4::HelpLabel,
            "안내!?🧭",
        ),
        (
            "모달 제목을 v1.0로 설정해줘.",
            IntentRecipeDetailFieldV4::ModalTitle,
            "v1.0",
        ),
    ] {
        assert_eq!(
            expectations(human),
            vec![(field, literal.to_owned())],
            "{human}"
        );
    }
    assert!(expectations("도움말 버튼 라벨을 안내로 변경해줘?").is_empty());
}

#[test]
fn full_custom_study_room_prompt_retains_only_material_serving_fields() {
    let human = "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'. Leave room closing disabled.";
    assert_eq!(
        fields(human),
        vec![
            IntentRecipeDetailFieldV4::CreateButtonLabel,
            IntentRecipeDetailFieldV4::ChannelNamePrefix,
            IntentRecipeDetailFieldV4::HelpLabel,
            IntentRecipeDetailFieldV4::HelpResponse,
        ]
    );
}

#[test]
fn analysis_fields_are_canonical_across_entries_and_omit_empty_counterparts() {
    let human = "Exact overrides: closed response is 'Closed'; channel name suffix is '-room'; launcher content is 'Create'; channel name uses an empty prefix; Help button label is 'Guide'.";
    assert_eq!(
        fields(human),
        vec![
            IntentRecipeDetailFieldV4::LauncherContent,
            IntentRecipeDetailFieldV4::ChannelNameSuffix,
            IntentRecipeDetailFieldV4::HelpLabel,
            IntentRecipeDetailFieldV4::ClosedResponse,
        ]
    );
}

#[test]
fn conflicting_or_absent_detail_analysis_has_no_material_fields() {
    for human in [
        "Set the Help button label to 'Guide'. Set the Help button label to 'Assist'.",
        "Build a managed private study-room automation with default copy and controls.",
        "channel name uses an empty suffix",
    ] {
        let analysis = analyze_private_study_room_details(human);
        assert!(
            analysis.facets().is_empty(),
            "unexpected facets for {human}"
        );
        assert!(
            analysis.fields().is_empty(),
            "unexpected fields for {human}"
        );
    }
}

fn is_private_study_room_detail_requirement(human: &str, requirement: &str) -> bool {
    analyze_private_study_room_details(human).explains_requirement(requirement)
}

fn assert_grounded_details(
    human: &str,
    expected: &[IntentRecipeDetailFacetV3],
    requirements: &[&str],
) {
    assert_eq!(facets(human), expected, "unexpected facets for {human}");
    for requirement in requirements {
        assert!(
            is_private_study_room_detail_requirement(human, requirement),
            "unsupported requirement {requirement} for {human}"
        );
    }
}

#[test]
fn gemma_override_phrases_are_supported_by_closed_entries() {
    let human = "Build a managed private study-room automation. Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'.";
    for requirement in [
        "created channel name uses prefix 'focus-' and an empty suffix",
        "ephemeral response is 'Read this first'",
        "launcher create-button label is 'Start focus room'",
        "room Help button label is 'Guide'",
    ] {
        assert!(is_private_study_room_detail_requirement(human, requirement));
    }
    assert_eq!(
        facets(human),
        vec![
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ]
    );
}

#[test]
fn evaluation_default_wrappers_preserve_exact_detail_evidence() {
    assert_grounded_details(
        "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except for generated names: the channel name has prefix 'focus-' and suffix '-room', and the member-role name has prefix 'team-' and suffix '-members'. Leave all copy and controls at their defaults, keep closing disabled, and do not ask a follow-up question.",
        &[IntentRecipeDetailFacetV3::Naming],
        &[
            "the channel name has prefix 'focus-' and suffix '-room'",
            "the member-role name has prefix 'team-' and suffix '-members'",
        ],
    );
    assert_grounded_details(
        "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except that the room Help button label is exactly 'Guide' and its ephemeral response is exactly 'Read the guide'. Keep default copy and naming, leave closing disabled, and do not ask a follow-up question.",
        &[IntentRecipeDetailFacetV3::Controls],
        &[
            "the room Help button label is exactly 'Guide'",
            "its ephemeral response is exactly 'Read the guide'",
        ],
    );
    assert_grounded_details(
        "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults except for these exact overrides: the launcher create-button label is 'Start focus room'; the created channel name uses prefix 'focus-' and an empty suffix; the room Help button label is 'Guide' and its ephemeral response is 'Read this first'. Leave room closing disabled.",
        &[
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ],
        &["its ephemeral response is 'Read this first'"],
    );
    assert_grounded_details(
        "Build a managed private study-room automation in community_hub and prepare its validated preview. Use English defaults for every name and room control, with exactly one copy override: set the launcher create-button label to 'Begin deep work'. Leave room closing disabled and do not ask a follow-up question.",
        &[IntentRecipeDetailFacetV3::Copy],
        &["launcher create-button label to 'Begin deep work'"],
    );
}

#[test]
fn closed_default_wrappers_enforce_facets_and_cardinality() {
    let valid_naming = "Use English defaults except for generated names:\n- the channel name has prefix 'focus-' and suffix '-room'\n- the member-role name has prefix 'team-' and suffix '-members'";
    assert_grounded_details(
        valid_naming,
        &[IntentRecipeDetailFacetV3::Naming],
        &[
            "the channel name has prefix 'focus-' and suffix '-room'",
            "the member-role name has prefix 'team-' and suffix '-members'",
        ],
    );

    let valid_copy = "Use English defaults for every name and room control, with exactly one copy override: set the launcher create-button label to 'Begin deep work'.";
    assert_grounded_details(
        valid_copy,
        &[IntentRecipeDetailFacetV3::Copy],
        &["launcher create-button label to 'Begin deep work'"],
    );

    let valid_scoped_facets = "Use default controls, except for copy and naming: set the modal title to 'Focus'; set the channel name prefix to 'focus-'.";
    assert_grounded_details(
        valid_scoped_facets,
        &[
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
        ],
        &["modal title to 'Focus'", "channel name prefix to 'focus-'"],
    );

    for invalid in [
        "Use English defaults except for generated names: the Help button label is 'Guide'.",
        "Use default copy and controls, except for naming: set the Help button label to 'Guide'.",
        "Use default copy and controls, except for naming:\n- set the Help button label to 'Guide'",
        "Use English defaults for every name and room control, with exactly one copy override:\n- the launcher create-button label is 'Begin'\n- the modal title is 'Focus room'",
        "Use English defaults except for generated names: when a room opens, set the channel name prefix to 'focus-'.",
        "Use English defaults for every name and room control, with exactly one copy override: do not set the launcher create-button label to 'Begin'.",
    ] {
        assert!(facets(invalid).is_empty(), "unexpected facets for {invalid}");
    }
}

#[test]
fn no_followup_meta_directives_do_not_weaken_scope_guards() {
    let safe =
        "Exact overrides: the Help button label is 'Guide'. Do not ask a follow-up question.";
    assert_grounded_details(
        safe,
        &[IntentRecipeDetailFacetV3::Controls],
        &["the Help button label is 'Guide'"],
    );

    for unsafe_request in [
        "Do not apply these overrides. Set the Help button label to 'Guide'.",
        "Exact overrides: the Help button label is 'Guide'. Do not ask a follow-up question and do not apply these overrides.",
        "Use English defaults except for generated names: the channel name prefix is 'focus-'; send the channel name suffix '-room' to a webhook.",
    ] {
        assert!(
            facets(unsafe_request).is_empty(),
            "unexpected facets for {unsafe_request}"
        );
    }
}

#[test]
fn dynamic_sentences_never_select_recipe_detail_facets() {
    for human in [
        "When the Close button is clicked, change the channel name to 'closed'.",
        "Set the channel name to 'closed' when the Close button is clicked.",
        "Upon a Close press, set the channel name to 'closed'.",
        "The score determines the channel name is 'winner'.",
        "The score can set the channel name to 'winner'.",
        "Set the channel name to 'closed' on-click.",
        "Set the channel name prefix to winner- on approval.",
        "For every message, send an ephemeral response 'Logged'.",
        "Add another panel whose launcher message is 'Second'.",
        "Create another panel with launcher message 'Second panel'.",
        "닫기 버튼을 누르면 채널 이름을 '종료'로 변경해.",
        "채널 이름 접두사는 '승인-'로 설정하면.",
        "채널명 'x'로 설정 금지.",
        "메시지마다 일회성 응답을 '기록'으로 설정해.",
    ] {
        assert!(facets(human).is_empty(), "unexpected facets for {human}");
    }
}

#[test]
fn dynamic_scope_cannot_reclassify_following_static_syntax() {
    for human in [
        "When clicked. Set the channel name prefix to 'closed-'.",
        "When the Close button is clicked by any room member. Set the channel name prefix to 'closed-'.",
        "When clicked:\n- channel name prefix is 'closed-'.",
        "Do not apply these overrides. Set the Help button label to 'Guide'.",
        "Don’t apply these overrides. Set the Help button label to 'Guide'.",
        "Omit these overrides. Set the Help button label to 'Guide'.",
        "Disable these overrides. Set the Help button label to 'Guide'.",
        "Unless approved. Set the channel name prefix to 'closed-'.",
        "Every room creation. Set the channel name prefix to 'closed-'.",
        "Please do not apply these overrides. Set the Help button label to 'Guide'.",
        "Please, do not apply these overrides. Set the Help button label to 'Guide'.",
        "Only after approval. Set the channel name prefix to 'closed-'.",
        "Only, after approval. Set the channel name prefix to 'closed-'.",
        "Hypothetically. Set the Help button label to 'Guide'.",
        "Suppose this were requested. Set the Help button label to 'Guide'.",
        "Imagine this were requested. Set the Help button label to 'Guide'.",
        "What if this were requested. Set the Help button label to 'Guide'.",
        "클릭하면:\n- 채널 이름 접두사는 '종료-'로 설정.",
        "이 설정은 적용하지 마. 도움말 버튼 라벨을 '안내'로 바꿔줘.",
    ] {
        assert!(facets(human).is_empty(), "unexpected facets for {human}");
    }
}

#[test]
fn static_detail_contexts_preserve_supported_language_variants() {
    for (human, expected) in [
        (
            "Set the Help button label to 'Guide me' and its Help response to 'Open the handbook'.",
            vec![IntentRecipeDetailFacetV3::Controls],
        ),
        (
            "Use default naming but set the channel name prefix to focus-.",
            vec![IntentRecipeDetailFacetV3::Naming],
        ),
        (
            "Use default copy and controls, except for naming: set the member role name prefix to 'team-'.",
            vec![IntentRecipeDetailFacetV3::Naming],
        ),
        (
            "채널 이름 접두사는 ‘공부-’로 설정하고 도움말 버튼 라벨은 「안내」로 바꿔줘.",
            vec![
                IntentRecipeDetailFacetV3::Naming,
                IntentRecipeDetailFacetV3::Controls,
            ],
        ),
        (
            "채널 이름 접두사를 focus-로 설정하고 모달 제목을 집중 방으로 변경해줘.",
            vec![
                IntentRecipeDetailFacetV3::Copy,
                IntentRecipeDetailFacetV3::Naming,
            ],
        ),
    ] {
        assert_eq!(facets(human), expected, "unexpected facets for {human}");
    }
}

#[test]
fn ambiguous_legacy_detail_phrasing_fails_closed() {
    for human in [
        "Do not use channel name prefix 'old-' but set it to 'fresh-'.",
        "Create private rooms with a Start exact focus button.",
        "«Help button label is 'Guide'»",
        "‹Help button label is 'Guide'›",
        "《Help button label is 'Guide'》",
        "> Help button label is 'Guide'",
    ] {
        assert!(facets(human).is_empty(), "unexpected facets for {human}");
    }
}

#[test]
fn questions_and_hypotheticals_never_become_static_assignments() {
    for human in [
        "Build a study room. Help button label is 'Guide'?",
        "도움말 버튼 라벨은 '안내'로 바꿔줄래？",
    ] {
        assert!(facets(human).is_empty(), "unexpected facets for {human}");
    }
}

#[test]
fn closed_override_headers_reject_negative_and_quantified_contexts() {
    for human in [
        "Avoid these exact overrides: the channel name is 'old'.",
        "Except these exact overrides: the channel name prefix is 'old-'.",
        "For every event, exact overrides: the channel name is 'event'.",
        "Upon activation, use these exact overrides: the channel name is 'active'.",
        "Do not apply these exact overrides: the channel name is 'old'.",
        "Use these exact overrides for 'when clicked': the channel name is 'closed'.",
        "Exact overrides 'do not apply': the channel name is 'closed'.",
        "’Exact overrides’: the channel name prefix is 'closed-'.",
        "”Exact overrides”: the channel name prefix is 'closed-'.",
        "승인 시 정확한 재정의: 채널명 '종료'로 설정",
        "절대 적용 금지인 정확한 재정의: 채널명 '종료'로 설정",
        "외부 웹훅 성공 시 정확한 재정의: 채널명 '종료'로 설정",
    ] {
        assert!(facets(human).is_empty(), "unexpected facets for {human}");
        assert!(!is_private_study_room_detail_requirement(
            human,
            "channel name is 'old'"
        ));
    }
}

#[test]
fn closed_override_headers_accept_only_unambiguous_templates() {
    for human in [
        "Exact overrides: the Help button label is 'Guide'.",
        "Use these exact overrides: the Help button label is 'Guide'.",
        "Apply the following exact overrides: the Help button label is 'Guide'.",
        "Use English defaults except for these exact overrides: the Help button label is 'Guide'.",
    ] {
        assert_eq!(
            facets(human),
            vec![IntentRecipeDetailFacetV3::Controls],
            "missing facet for {human}"
        );
    }
}

#[test]
fn command_words_inside_a_typed_literal_remain_static_data() {
    let human = "Exact overrides: the Help button label is 'Do not panic'.";
    assert_eq!(facets(human), vec![IntentRecipeDetailFacetV3::Controls]);
    assert!(is_private_study_room_detail_requirement(
        human,
        "Help button label is 'Do not panic'"
    ));
}

#[test]
fn korean_static_assignments_are_typed_for_facets_and_requirement_coverage() {
    let human = "채널 이름 접두사는 '공부-'로 설정하고 도움말 버튼 라벨은 '안내'로 바꿔줘.";
    assert_eq!(
        facets(human),
        vec![
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ]
    );
    for requirement in [
        "채널 이름 접두사는 '공부-'로 설정",
        "도움말 버튼 라벨은 '안내'로 바꿔줘",
    ] {
        assert!(is_private_study_room_detail_requirement(human, requirement));
    }
}

#[test]
fn room_panel_control_wording_is_a_static_control_detail() {
    let human = "Set the room panel Help button label to 'Guide'.";
    assert_eq!(facets(human), vec![IntentRecipeDetailFacetV3::Controls]);
    assert!(is_private_study_room_detail_requirement(
        human,
        "room panel Help button label to 'Guide'"
    ));
}

#[test]
fn supported_detail_phrases_stay_inside_their_typed_entries() {
    for (human, requirement) in [
        (
            "Set the room Help button label to 'Guide' and its ephemeral response is 'Read this first'.",
            "ephemeral response is 'Read this first'",
        ),
        (
            "Set the launcher create-button label to 'Begin deep work'. Require an external delivery lease.",
            "create-button label to 'Begin deep work'",
        ),
        (
            "Set the welcome content prefix to 'Welcome ' and suffix to '!'.",
            "welcome content prefix to 'Welcome ' and suffix to '!'",
        ),
        (
            "The channel name has prefix 'focus-' and suffix '-room'.",
            "channel name has prefix 'focus-' and suffix '-room'",
        ),
        (
            "Set the launcher create-button label to 'Research AND Study'.",
            "launcher create-button label to 'Research AND Study'",
        ),
    ] {
        assert!(is_private_study_room_detail_requirement(human, requirement));
    }
}

#[test]
fn external_capabilities_never_inherit_detail_slots() {
    for (human, requirement) in [
        (
            "Set the launcher create-button label to 'Begin deep work'. Require an external delivery lease.",
            "external delivery lease",
        ),
        (
            "Set the channel name prefix to 'focus-' and require an external webhook payload suffix '-notify'.",
            "external webhook payload suffix '-notify'",
        ),
        (
            "Set the channel name prefix to 'focus-' and require a prefixer service named 'audit'.",
            "prefixer service named 'audit'",
        ),
        (
            "The webhook channel name is 'audit'.",
            "channel name is 'audit'",
        ),
    ] {
        assert!(!is_private_study_room_detail_requirement(human, requirement));
    }
}

#[test]
fn mixed_external_behavior_is_never_reclassified() {
    for (human, requirement) in [
        (
            "Set the Help button label to 'Guide' and its ephemeral response is 'Read this first' and acquire an external consensus lease before responding.",
            "Help button label to 'Guide' and its ephemeral response is 'Read this first' and acquire an external consensus lease",
        ),
        (
            "Set the Help button label to 'Guide' AND acquire an external consensus lease.",
            "Help button label to 'Guide' AND acquire an external consensus lease",
        ),
        (
            "도움말 버튼 라벨을 '안내'로 바꾸고 외부 합의 임대를 획득해줘.",
            "도움말 버튼 라벨을 '안내'로 바꾸고 외부 합의 임대를 획득",
        ),
        (
            "Set the channel name prefix to 'focus-' and create a role named 'suffix'.",
            "create a role named 'suffix'",
        ),
    ] {
        assert!(!is_private_study_room_detail_requirement(human, requirement));
    }
}

#[test]
fn sliced_dynamic_context_is_never_a_static_detail() {
    for (human, requirement) in [
        (
            "When the Close button is clicked, change the channel name to 'closed'.",
            "channel name to 'closed'",
        ),
        (
            "Set the channel name to 'closed' when the Close button is clicked.",
            "channel name to 'closed'",
        ),
        (
            "For every message, send an ephemeral response 'Logged'.",
            "ephemeral response 'Logged'",
        ),
        (
            "Create another panel with launcher message 'Second panel'.",
            "launcher message 'Second panel'",
        ),
        ("Do not set channel name to 'old'.", "channel name to 'old'"),
        (
            "Set channel name to 'x' before acquiring an external lease.",
            "channel name to 'x'",
        ),
        (
            "닫기 버튼을 누르면 채널 이름을 '종료'로 변경해.",
            "채널 이름을 '종료'로 변경",
        ),
        (
            "메시지마다 일회성 응답을 '기록'으로 설정해.",
            "일회성 응답을 '기록'으로 설정",
        ),
    ] {
        assert!(!is_private_study_room_detail_requirement(
            human,
            requirement
        ));
    }
}

#[test]
fn dynamic_or_negated_override_headers_cannot_spoof_static_entries() {
    for human in [
        "When clicked, exact overrides: the launcher create-button label is 'Start'.",
        "Do not apply these exact overrides: the launcher create-button label is 'Start'.",
    ] {
        assert!(!is_private_study_room_detail_requirement(
            human,
            "launcher create-button label is 'Start'"
        ));
    }
}

#[test]
fn one_unsupported_override_entry_rejects_the_whole_list() {
    let human = "Exact overrides: the Help button label is 'Guide'; send the channel name prefix 'focus-' to a webhook.";
    assert!(!is_private_study_room_detail_requirement(
        human,
        "Help button label is 'Guide'"
    ));
}

#[test]
fn multiline_override_lists_are_all_or_nothing() {
    let valid =
        "Exact overrides:\n- Help button label is 'Guide'\n- channel name prefix is 'focus-'";
    assert_eq!(
        facets(valid),
        vec![
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ]
    );
    for requirement in [
        "Help button label is 'Guide'",
        "channel name prefix is 'focus-'",
    ] {
        assert!(is_private_study_room_detail_requirement(valid, requirement));
    }

    let valid_with_spacing =
        "Exact overrides:\n\n- Help button label is 'Guide'\n- channel name prefix is 'focus-'";
    assert_eq!(
        facets(valid_with_spacing),
        vec![
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ]
    );

    let valid_default_header =
        "Use English defaults except for these exact overrides:\n- Help button label is 'Guide'";
    assert_eq!(
        facets(valid_default_header),
        vec![IntentRecipeDetailFacetV3::Controls]
    );

    let valid_numbered =
        "Exact overrides:\n1. Help button label is 'Guide'\n2. channel name prefix is 'focus-'";
    assert_eq!(
        facets(valid_numbered),
        vec![
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ]
    );
    assert!(is_private_study_room_detail_requirement(
        valid_numbered,
        "Help button label is 'Guide'"
    ));

    let mixed = "Set the modal title to 'Focus'.\nExact overrides:\n- channel name prefix is 'focus-'\n\nSet the Help button label to 'Guide'.";
    assert_eq!(
        facets(mixed),
        vec![
            IntentRecipeDetailFacetV3::Copy,
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Controls,
        ]
    );

    let invalid = "Exact overrides:\n- Help button label is 'Guide'\n- send channel prefix 'focus-' to a webhook";
    assert!(facets(invalid).is_empty());
    assert!(!is_private_study_room_detail_requirement(
        invalid,
        "Help button label is 'Guide'"
    ));

    let question = "Exact overrides:\n- Help button label is 'Guide'?";
    assert!(facets(question).is_empty());

    for invalid_header in [
        "Do not apply these exact overrides:\n- Help button label is 'Guide'",
        "’Exact overrides’:\n- Help button label is 'Guide'",
        "’Exact overrides’:\n\n- Help button label is 'Guide'",
        "Untrusted details:\n- Help button label is 'Guide'",
    ] {
        assert!(
            facets(invalid_header).is_empty(),
            "unexpected facets for {invalid_header}"
        );
    }
}

#[test]
fn every_occurrence_must_have_safe_static_context() {
    let mixed = "Exact overrides: the Help button label is 'Guide'. When clicked, set the Help button label is 'Guide'.";
    assert!(!is_private_study_room_detail_requirement(
        mixed,
        "Help button label is 'Guide'"
    ));

    let safe = "Set the Help button label to 'Guide'. Set the Help button label to 'Guide'.";
    assert!(is_private_study_room_detail_requirement(
        safe,
        "Help button label to 'Guide'"
    ));
}

#[test]
fn repeated_static_evidence_uses_the_same_closed_occurrence_contract() {
    let human = std::iter::repeat_n("Set the Help button label to 'Guide'.", 512)
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(facets(&human), vec![IntentRecipeDetailFacetV3::Controls]);
    assert!(is_private_study_room_detail_requirement(
        &human,
        "Help button label to 'Guide'"
    ));
}

#[test]
fn conflicting_assignments_fail_closed_across_entry_boundaries() {
    for human in [
        "Set the Help button label to 'Guide'. Set the Help button label to 'Assist'.",
        "Exact overrides: Help button label is 'Guide'; Help button label is 'Assist'.",
        "Exact overrides:\n- Help button label is 'Guide'\n- Help button label is 'Assist'",
    ] {
        assert!(facets(human).is_empty(), "unexpected facets for {human}");
    }

    let compatible =
        "Exact overrides: channel name prefix is 'focus-'; channel name uses an empty suffix.";
    assert_eq!(facets(compatible), vec![IntentRecipeDetailFacetV3::Naming]);
}

#[test]
fn literal_position_and_count_are_closed() {
    for (human, requirement) in [
        (
            "Use the 'external' channel name.",
            "'external' channel name",
        ),
        (
            "Set the channel name prefix to 'focus-' 'shadow-'.",
            "channel name prefix to 'focus-'",
        ),
        (
            "Set the channel name prefix to 'focus-' and 'shadow-'.",
            "channel name prefix to 'focus-' and 'shadow-'",
        ),
        (
            "Use 'Guide' Help button label is 'Shown'.",
            "Help button label is 'Shown'",
        ),
    ] {
        assert!(!is_private_study_room_detail_requirement(
            human,
            requirement
        ));
    }
}

#[test]
fn semantic_connectors_are_not_deletion_connectors() {
    for connector in ["before", "after", "while", "then"] {
        let human = format!("Set the Help button label to 'Guide' {connector} creating a role.");
        assert!(!is_private_study_room_detail_requirement(
            &human,
            "Help button label to 'Guide'"
        ));
    }
}

#[test]
fn default_nonexact_and_empty_requirements_are_preserved() {
    let human = "Use the default channel name and the default Help response.";
    for requirement in ["channel name", "Channel name", "   "] {
        assert!(!is_private_study_room_detail_requirement(
            human,
            requirement
        ));
    }
}
