use super::intent_core::IntentRecipeDetailFacetV3;
use super::intent_detail_syntax::{closed_detail_syntax_tokens, LITERAL_SENTINEL};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetailSlot {
    LauncherContent,
    CreateButtonLabel,
    ModalTitle,
    RoomNameLabel,
    WelcomeContent,
    HubAnnouncement,
    CompletedResponse,
    ChannelName,
    MemberRoleName,
    HelpLabel,
    HelpResponse,
    JoinLabel,
    JoinResponse,
    CloseLabel,
    CloseResponse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetailValueShape {
    Direct,
    Affix,
}

impl DetailSlot {
    pub(super) fn facet(self) -> IntentRecipeDetailFacetV3 {
        match self {
            Self::LauncherContent
            | Self::CreateButtonLabel
            | Self::ModalTitle
            | Self::RoomNameLabel
            | Self::WelcomeContent
            | Self::HubAnnouncement
            | Self::CompletedResponse => IntentRecipeDetailFacetV3::Copy,
            Self::ChannelName | Self::MemberRoleName => IntentRecipeDetailFacetV3::Naming,
            Self::HelpLabel
            | Self::HelpResponse
            | Self::JoinLabel
            | Self::JoinResponse
            | Self::CloseLabel
            | Self::CloseResponse => IntentRecipeDetailFacetV3::Controls,
        }
    }

    pub(super) fn value_shape(self) -> DetailValueShape {
        match self {
            Self::WelcomeContent
            | Self::HubAnnouncement
            | Self::CompletedResponse
            | Self::ChannelName
            | Self::MemberRoleName => DetailValueShape::Affix,
            _ => DetailValueShape::Direct,
        }
    }

    fn response_slot(self) -> Option<Self> {
        match self {
            Self::HelpLabel | Self::HelpResponse => Some(Self::HelpResponse),
            Self::JoinLabel | Self::JoinResponse => Some(Self::JoinResponse),
            Self::CloseLabel | Self::CloseResponse => Some(Self::CloseResponse),
            _ => None,
        }
    }
}

pub(super) fn parse_detail_requirement_segment(
    segment: &str,
    active_slot: Option<DetailSlot>,
) -> Option<DetailSlot> {
    let tokens = closed_detail_syntax_tokens(segment)?;
    let tokens = tokens.iter().map(String::as_str).collect::<Vec<_>>();
    let has_literal = tokens.contains(&LITERAL_SENTINEL);
    let has_empty_value = tokens
        .iter()
        .any(|token| matches!(*token, "empty" | "빈" | "비운" | "비어"));
    if !has_literal && !has_empty_value {
        return parse_unquoted_detail_requirement_segment(&tokens, active_slot);
    }
    let tokens = strip_detail_command_prefix(&tokens);
    if let Some((slot, tail)) = match_detail_slot(tokens) {
        return valid_detail_assignment(slot, tail).then_some(slot);
    }
    let active_slot = active_slot?;
    match_detail_continuation(tokens, active_slot)
}

fn parse_unquoted_detail_requirement_segment(
    tokens: &[&str],
    active_slot: Option<DetailSlot>,
) -> Option<DetailSlot> {
    let tokens = strip_detail_command_prefix(tokens);
    if let Some((slot, tail)) = match_detail_slot(tokens) {
        return valid_unquoted_detail_assignment(slot, tail).then_some(slot);
    }
    let active_slot = active_slot?;
    valid_unquoted_detail_continuation(tokens, active_slot).then_some(active_slot)
}

fn valid_unquoted_detail_assignment(slot: DetailSlot, tail: &[&str]) -> bool {
    match slot.value_shape() {
        DetailValueShape::Direct => valid_unquoted_direct(tail),
        DetailValueShape::Affix => valid_unquoted_affix(tail),
    }
}

fn valid_unquoted_direct(tail: &[&str]) -> bool {
    let value = match tail {
        ["is", value]
        | ["are", value]
        | ["to", value]
        | ["as", value]
        | ["named", value]
        | ["set", "to", value] => Some(*value),
        _ => None,
    };
    value.is_some_and(valid_unquoted_value_token) || korean_unquoted_direct(tail)
}

fn valid_unquoted_value_token(value: &str) -> bool {
    !matches!(
        value,
        "after"
            | "before"
            | "click"
            | "clicked"
            | "create"
            | "default"
            | "defaults"
            | "disable"
            | "disabled"
            | "each"
            | "every"
            | "if"
            | "never"
            | "none"
            | "not"
            | "off"
            | "omit"
            | "omitted"
            | "per"
            | "prohibited"
            | "send"
            | "standard"
            | "then"
            | "trigger"
            | "unchanged"
            | "unless"
            | "when"
            | "whenever"
            | "while"
            | "without"
            | "기본"
            | "기본값"
            | "금지"
            | "forbidden"
    ) && ![
        "기본",
        "금지",
        "누르면",
        "마다",
        "않",
        "없이",
        "하면",
        "되면",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn korean_unquoted_direct(tail: &[&str]) -> bool {
    const TERMINALS: &[&str] = &[
        "바꿔",
        "바꿔줘",
        "바꿔주세요",
        "바꾸고",
        "변경",
        "변경해",
        "변경해줘",
        "설정",
        "설정해",
        "설정해줘",
    ];
    let Some((terminal, value)) = tail.split_last() else {
        return false;
    };
    if !TERMINALS.contains(terminal)
        || value.is_empty()
        || value.len() > 2
        || !value.iter().all(|token| valid_unquoted_value_token(token))
    {
        return false;
    }
    value
        .last()
        .is_some_and(|token| *token == "로" || token.ends_with("로") || token.ends_with("으로"))
}

fn valid_unquoted_affix(tail: &[&str]) -> bool {
    let value = match tail {
        ["uses", "prefix", value]
        | ["uses", "suffix", value]
        | ["has", "prefix", value]
        | ["has", "suffix", value]
        | ["with", "prefix", value]
        | ["with", "suffix", value]
        | ["prefix", value]
        | ["suffix", value]
        | ["prefix", "is", value]
        | ["suffix", "is", value]
        | ["prefix", "to", value]
        | ["suffix", "to", value] => Some(*value),
        _ => None,
    };
    value.is_some_and(valid_unquoted_value_token) || korean_unquoted_affix(tail)
}

fn korean_unquoted_affix(tail: &[&str]) -> bool {
    let Some((affix, value)) = tail.split_first() else {
        return false;
    };
    if !matches!(
        *affix,
        "접두사" | "접두사는" | "접두사를" | "접미사" | "접미사는" | "접미사를"
    ) {
        return false;
    }
    matches!(value, [literal, "로", "설정"] if valid_unquoted_value_token(literal))
}

fn valid_unquoted_detail_continuation(tokens: &[&str], active_slot: DetailSlot) -> bool {
    match active_slot.value_shape() {
        DetailValueShape::Direct => false,
        DetailValueShape::Affix => valid_unquoted_affix(tokens),
    }
}

pub(super) fn strip_detail_command_prefix<'a>(tokens: &'a [&str]) -> &'a [&'a str] {
    const PREFIXES: &[&[&str]] = &[
        &["set", "the"],
        &["set"],
        &["use", "the"],
        &["use"],
        &["change", "the"],
        &["change"],
        &["customize", "the"],
        &["customize"],
        &["override", "the"],
        &["override"],
        &["rename", "the"],
        &["rename"],
        &["설정"],
        &["변경"],
        &["지정"],
        &["재정의"],
    ];
    for prefix in PREFIXES {
        if tokens.starts_with(prefix) {
            return &tokens[prefix.len()..];
        }
    }
    tokens
        .strip_prefix(&["the"])
        .or_else(|| tokens.strip_prefix(&["a"]))
        .or_else(|| tokens.strip_prefix(&["an"]))
        .unwrap_or(tokens)
}

fn match_detail_slot<'a>(tokens: &'a [&'a str]) -> Option<(DetailSlot, &'a [&'a str])> {
    const PATTERNS: &[(DetailSlot, &[&str])] = &[
        (
            DetailSlot::CreateButtonLabel,
            &["launcher", "create", "button", "label"],
        ),
        (
            DetailSlot::CreateButtonLabel,
            &["create", "button", "label"],
        ),
        (DetailSlot::LauncherContent, &["launcher", "content"]),
        (DetailSlot::LauncherContent, &["launcher", "copy"]),
        (DetailSlot::LauncherContent, &["launcher", "message"]),
        (DetailSlot::LauncherContent, &["launcher", "text"]),
        (DetailSlot::ModalTitle, &["modal", "title"]),
        (DetailSlot::RoomNameLabel, &["room", "name", "label"]),
        (DetailSlot::WelcomeContent, &["welcome", "content"]),
        (DetailSlot::WelcomeContent, &["welcome", "copy"]),
        (DetailSlot::WelcomeContent, &["welcome", "message"]),
        (DetailSlot::HubAnnouncement, &["hub", "announcement"]),
        (DetailSlot::CompletedResponse, &["completion", "response"]),
        (DetailSlot::CompletedResponse, &["completed", "response"]),
        (DetailSlot::ChannelName, &["created", "channel", "name"]),
        (DetailSlot::ChannelName, &["channel", "name"]),
        (DetailSlot::MemberRoleName, &["member", "role", "name"]),
        (DetailSlot::MemberRoleName, &["member", "name", "pattern"]),
        (DetailSlot::MemberRoleName, &["role", "name", "pattern"]),
        (
            DetailSlot::HelpLabel,
            &["room", "panel", "help", "button", "label"],
        ),
        (DetailSlot::HelpLabel, &["room", "help", "button", "label"]),
        (DetailSlot::HelpLabel, &["help", "button", "label"]),
        (DetailSlot::HelpLabel, &["help", "label"]),
        (
            DetailSlot::HelpResponse,
            &["room", "panel", "help", "response"],
        ),
        (DetailSlot::HelpResponse, &["help", "response"]),
        (DetailSlot::HelpResponse, &["help", "message"]),
        (DetailSlot::JoinLabel, &["room", "join", "button", "label"]),
        (
            DetailSlot::JoinLabel,
            &["room", "panel", "join", "button", "label"],
        ),
        (DetailSlot::JoinLabel, &["join", "button", "label"]),
        (DetailSlot::JoinLabel, &["join", "label"]),
        (DetailSlot::JoinResponse, &["join", "response"]),
        (
            DetailSlot::JoinResponse,
            &["room", "panel", "join", "response"],
        ),
        (DetailSlot::JoinResponse, &["joined", "response"]),
        (
            DetailSlot::CloseLabel,
            &["room", "close", "button", "label"],
        ),
        (
            DetailSlot::CloseLabel,
            &["room", "panel", "close", "button", "label"],
        ),
        (DetailSlot::CloseLabel, &["close", "button", "label"]),
        (DetailSlot::CloseLabel, &["close", "label"]),
        (DetailSlot::CloseResponse, &["close", "response"]),
        (
            DetailSlot::CloseResponse,
            &["room", "panel", "close", "response"],
        ),
        (DetailSlot::CloseResponse, &["closed", "response"]),
        (
            DetailSlot::CreateButtonLabel,
            &["런처", "만들기", "버튼", "라벨을"],
        ),
        (
            DetailSlot::CreateButtonLabel,
            &["런처", "생성", "버튼", "라벨을"],
        ),
        (DetailSlot::ModalTitle, &["모달", "제목을"]),
        (DetailSlot::ChannelName, &["채널", "이름"]),
        (DetailSlot::ChannelName, &["채널명"]),
        (DetailSlot::MemberRoleName, &["멤버", "역할", "이름"]),
        (DetailSlot::MemberRoleName, &["멤버", "역할명"]),
        (DetailSlot::HelpLabel, &["도움말", "버튼", "라벨을"]),
        (DetailSlot::HelpLabel, &["도움말", "버튼", "라벨은"]),
        (DetailSlot::HelpResponse, &["도움말", "응답을"]),
        (DetailSlot::JoinLabel, &["참가", "버튼", "라벨을"]),
        (DetailSlot::JoinLabel, &["참여", "버튼", "라벨을"]),
        (DetailSlot::JoinResponse, &["참가", "응답을"]),
        (DetailSlot::JoinResponse, &["참여", "응답을"]),
        (DetailSlot::CloseLabel, &["닫기", "버튼", "라벨을"]),
        (DetailSlot::CloseLabel, &["종료", "버튼", "라벨을"]),
        (DetailSlot::CloseResponse, &["닫기", "응답을"]),
        (DetailSlot::CloseResponse, &["종료", "응답을"]),
    ];
    PATTERNS
        .iter()
        .find_map(|(slot, pattern)| tokens.strip_prefix(*pattern).map(|tail| (*slot, tail)))
}

fn valid_detail_assignment(slot: DetailSlot, tail: &[&str]) -> bool {
    const DIRECT: &[&[&str]] = &[
        &[LITERAL_SENTINEL],
        &["is", LITERAL_SENTINEL],
        &["are", LITERAL_SENTINEL],
        &["to", LITERAL_SENTINEL],
        &["set", "to", LITERAL_SENTINEL],
        &["is", "set", "to", LITERAL_SENTINEL],
        &["as", LITERAL_SENTINEL],
        &["named", LITERAL_SENTINEL],
        &[LITERAL_SENTINEL, "로", "바꿔"],
        &[LITERAL_SENTINEL, "로", "바꿔줘"],
        &[LITERAL_SENTINEL, "로", "바꿔주세요"],
        &[LITERAL_SENTINEL, "로", "바꾸고"],
        &[LITERAL_SENTINEL, "로", "변경"],
        &[LITERAL_SENTINEL, "로", "변경해"],
        &[LITERAL_SENTINEL, "로", "변경해줘"],
        &[LITERAL_SENTINEL, "로", "설정"],
        &[LITERAL_SENTINEL, "로", "설정해"],
        &[LITERAL_SENTINEL, "로", "설정해줘"],
    ];
    const AFFIX: &[&[&str]] = &[
        &["uses", "prefix", LITERAL_SENTINEL],
        &["uses", "suffix", LITERAL_SENTINEL],
        &["has", "prefix", LITERAL_SENTINEL],
        &["has", "suffix", LITERAL_SENTINEL],
        &["with", "prefix", LITERAL_SENTINEL],
        &["with", "suffix", LITERAL_SENTINEL],
        &["prefix", LITERAL_SENTINEL],
        &["suffix", LITERAL_SENTINEL],
        &["prefix", "is", LITERAL_SENTINEL],
        &["suffix", "is", LITERAL_SENTINEL],
        &["prefix", "to", LITERAL_SENTINEL],
        &["suffix", "to", LITERAL_SENTINEL],
        &["uses", "an", "empty", "prefix"],
        &["uses", "an", "empty", "suffix"],
        &["uses", "empty", "prefix"],
        &["uses", "empty", "suffix"],
        &["has", "an", "empty", "prefix"],
        &["has", "an", "empty", "suffix"],
        &["has", "empty", "prefix"],
        &["has", "empty", "suffix"],
        &["with", "an", "empty", "prefix"],
        &["with", "an", "empty", "suffix"],
        &["with", "empty", "prefix"],
        &["with", "empty", "suffix"],
        &["an", "empty", "prefix"],
        &["an", "empty", "suffix"],
        &["empty", "prefix"],
        &["empty", "suffix"],
        &["접두사", LITERAL_SENTINEL],
        &["접두사", LITERAL_SENTINEL, "로", "설정"],
        &["접두사는", LITERAL_SENTINEL],
        &["접두사는", LITERAL_SENTINEL, "로", "설정"],
        &["접두사를", LITERAL_SENTINEL],
        &["접두사를", LITERAL_SENTINEL, "로", "설정"],
        &["접미사", LITERAL_SENTINEL],
        &["접미사", LITERAL_SENTINEL, "로", "설정"],
        &["접미사는", LITERAL_SENTINEL],
        &["접미사는", LITERAL_SENTINEL, "로", "설정"],
        &["접미사를", LITERAL_SENTINEL],
        &["접미사를", LITERAL_SENTINEL, "로", "설정"],
    ];
    match slot.value_shape() {
        DetailValueShape::Direct => DIRECT.contains(&tail),
        DetailValueShape::Affix => AFFIX.contains(&tail),
    }
}

fn match_detail_continuation(tokens: &[&str], active_slot: DetailSlot) -> Option<DetailSlot> {
    if active_slot.value_shape() == DetailValueShape::Affix
        && matches!(
            tokens,
            ["prefix", LITERAL_SENTINEL]
                | ["suffix", LITERAL_SENTINEL]
                | ["prefix", "is", LITERAL_SENTINEL]
                | ["suffix", "is", LITERAL_SENTINEL]
                | ["prefix", "to", LITERAL_SENTINEL]
                | ["suffix", "to", LITERAL_SENTINEL]
                | ["an", "empty", "prefix"]
                | ["an", "empty", "suffix"]
                | ["empty", "prefix"]
                | ["empty", "suffix"]
                | ["접두사", LITERAL_SENTINEL]
                | ["접두사는", LITERAL_SENTINEL]
                | ["접두사를", LITERAL_SENTINEL]
                | ["접미사", LITERAL_SENTINEL]
                | ["접미사는", LITERAL_SENTINEL]
                | ["접미사를", LITERAL_SENTINEL]
        )
    {
        return Some(active_slot);
    }
    if matches!(
        tokens,
        ["its", "response", LITERAL_SENTINEL]
            | ["its", "response", "is", LITERAL_SENTINEL]
            | ["its", "response", "to", LITERAL_SENTINEL]
            | ["its", "help", "response", LITERAL_SENTINEL]
            | ["its", "help", "response", "is", LITERAL_SENTINEL]
            | ["its", "help", "response", "to", LITERAL_SENTINEL]
            | ["its", "ephemeral", "response", LITERAL_SENTINEL]
            | ["its", "ephemeral", "response", "is", LITERAL_SENTINEL]
            | ["its", "ephemeral", "response", "to", LITERAL_SENTINEL]
    ) {
        return active_slot.response_slot();
    }
    None
}
