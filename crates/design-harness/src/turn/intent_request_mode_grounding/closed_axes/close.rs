use crate::turn::intent_interpretation::CloseAuthorizationV2;

use super::super::patterns::KOREAN_TARGET_PARTICLES;
use super::{syntax::*, AxisDirective};

pub(super) fn close_directive(
    value: &str,
    words: &[&str],
    inherited_close_scope: bool,
) -> AxisDirective<CloseAuthorizationV2> {
    let disabled = disabled_close_directive(value, words);
    let creator_only = creator_close_directive(value, words, inherited_close_scope)
        && !negative_close_scope(value, words);
    let any_member = any_member_close_directive(value, words, inherited_close_scope)
        && !negative_close_scope(value, words);
    match [disabled, any_member, creator_only]
        .into_iter()
        .filter(|selected| *selected)
        .count()
    {
        0 => AxisDirective::None,
        1 if disabled => AxisDirective::Value(CloseAuthorizationV2::Disabled),
        1 if any_member => AxisDirective::Value(CloseAuthorizationV2::AnyMember),
        1 => AxisDirective::Value(CloseAuthorizationV2::CreatorOnly),
        _ => AxisDirective::Conflict,
    }
}

pub(super) fn merge_alternative_close_branch(
    directive: AxisDirective<CloseAuthorizationV2>,
    previous: Option<CloseAuthorizationV2>,
    branch: AxisDirective<CloseAuthorizationV2>,
    alternative: bool,
) -> AxisDirective<CloseAuthorizationV2> {
    if !alternative {
        return directive;
    }
    let current = match (directive, branch) {
        (AxisDirective::Conflict, _) | (_, AxisDirective::Conflict) => {
            return AxisDirective::Conflict;
        }
        (AxisDirective::Value(left), AxisDirective::Value(right)) if left != right => {
            return AxisDirective::Conflict;
        }
        (AxisDirective::Value(value), _) | (_, AxisDirective::Value(value)) => Some(value),
        (AxisDirective::None, AxisDirective::None) => None,
    };
    match (previous, current) {
        (Some(left), Some(right)) if left != right => AxisDirective::Conflict,
        (_, Some(value)) => AxisDirective::Value(value),
        _ => AxisDirective::None,
    }
}

pub(super) fn close_branch_hint(
    value: &str,
    words: &[&str],
    inherited_close_scope: bool,
    alternative: bool,
) -> AxisDirective<CloseAuthorizationV2> {
    let any_member = english_any_member_close(words)
        || (["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"]
            .iter()
            .any(|marker| value.contains(marker))
            && korean_close_permission(value))
        || incomplete_any_member_branch(value, words);
    let creator_only = creator_close_directive(value, words, inherited_close_scope)
        || (alternative && inherited_close_scope && starts_with_creator_branch(value, words))
        || ((inherited_close_scope || direct_close_scope(value, words))
            && incomplete_creator_branch(value, words));
    let shared_inline_scope = alternative && direct_close_scope(value, words);
    let any_member = any_member
        || (shared_inline_scope
            && has_alternative_connector(value, words)
            && english_member_scope(words).is_some()
            && has_creator_actor(value, words));
    let creator_only = creator_only
        || (shared_inline_scope
            && has_alternative_connector(value, words)
            && has_creator_actor(value, words)
            && has_any_member_actor(value, words));
    match (any_member, creator_only) {
        (true, true) => AxisDirective::Conflict,
        (true, false) => AxisDirective::Value(CloseAuthorizationV2::AnyMember),
        (false, true) => AxisDirective::Value(CloseAuthorizationV2::CreatorOnly),
        (false, false) => AxisDirective::None,
    }
}

fn incomplete_any_member_branch(value: &str, words: &[&str]) -> bool {
    let english = english_member_scope(words).is_some_and(|(start, end)| {
        end == words.len()
            && (start == 0
                || words[..start]
                    .first()
                    .is_some_and(|word| matches!(*word, "allow" | "enable" | "let")))
    });
    let trimmed = value.trim();
    let korean = ["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"].contains(&trimmed);
    english || korean
}

fn incomplete_creator_branch(value: &str, words: &[&str]) -> bool {
    let english = words == ["only", "the", "room", "creator"]
        || words == ["only", "room", "creator"]
        || words == ["only", "the", "creator"]
        || words == ["only", "creator"];
    let trimmed = value.trim();
    let korean = ["만든 사람만", "방을 만든 사람만", "방 생성자만", "방장만"].contains(&trimmed);
    english || korean
}

fn starts_with_creator_branch(value: &str, words: &[&str]) -> bool {
    words.starts_with(&["only", "the", "room", "creator"])
        || words.starts_with(&["only", "room", "creator"])
        || words.starts_with(&["only", "the", "creator"])
        || words.starts_with(&["only", "creator"])
        || ["만든 사람만", "방 생성자만", "방장만"]
            .iter()
            .any(|marker| value.starts_with(marker))
}

fn has_creator_actor(value: &str, words: &[&str]) -> bool {
    contains_sequence(words, &["only", "the", "room", "creator"])
        || contains_sequence(words, &["only", "room", "creator"])
        || contains_sequence(words, &["only", "the", "creator"])
        || contains_sequence(words, &["only", "creator"])
        || ["만든 사람만", "방 생성자만", "방장만"]
            .iter()
            .any(|marker| value.contains(marker))
}

fn has_any_member_actor(value: &str, words: &[&str]) -> bool {
    english_member_scope(words).is_some()
        || ["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"]
            .iter()
            .any(|marker| value.contains(marker))
}

fn disabled_close_directive(value: &str, words: &[&str]) -> bool {
    let direct_words = strip_directive_prefixes(words);
    let english = direct_words.starts_with(&["leave", "closing", "disabled"])
        || direct_words.starts_with(&["leave", "room", "closing", "disabled"])
        || direct_words.starts_with(&["leave", "closing", "turned", "off"])
        || direct_words.starts_with(&["leave", "room", "closing", "turned", "off"])
        || direct_words.starts_with(&["leave", "the", "close", "button", "disabled"])
        || direct_words.starts_with(&["leave", "close", "button", "disabled"])
        || direct_words.starts_with(&["keep", "closing", "disabled"])
        || direct_words.starts_with(&["keep", "room", "closing", "disabled"])
        || direct_words.starts_with(&["keep", "closing", "turned", "off"])
        || direct_words.starts_with(&["keep", "room", "closing", "turned", "off"])
        || direct_words.starts_with(&["keep", "the", "close", "button", "disabled"])
        || direct_words.starts_with(&["keep", "close", "button", "disabled"])
        || direct_words.starts_with(&["the", "close", "button", "must", "remain", "disabled"])
        || direct_words.starts_with(&["the", "close", "button", "should", "remain", "disabled"])
        || direct_words.starts_with(&["closing", "is", "disabled"])
        || direct_words.starts_with(&["room", "closing", "is", "disabled"])
        || direct_words.starts_with(&["never", "enable", "closing"])
        || direct_words.starts_with(&["never", "enable", "room", "closing"])
        || direct_words.starts_with(&["do", "not", "add", "room", "closing"])
        || matches!(
            direct_words,
            ["don't" | "don’t" | "dont", "add", "room", "closing", ..]
        )
        || (direct_words
            .first()
            .is_some_and(|word| matches!(*word, "disable" | "omit" | "remove"))
            && (has_close_control(direct_words)
                || contains_sequence(direct_words, &["room", "closing"])))
        || (has_close_control(direct_words)
            && (direct_words.starts_with(&["do", "not", "add"])
                || direct_words.starts_with(&["do", "not", "enable"])
                || direct_words.starts_with(&["do", "not", "include"])
                || direct_words.starts_with(&["do", "not", "use"])
                || matches!(
                    direct_words,
                    [
                        "don't" | "don’t" | "dont",
                        "add" | "enable" | "include" | "use",
                        ..
                    ]
                )))
        || (direct_words.starts_with(&["do", "not", "allow", "anyone", "to", "close"])
            && direct_close_scope(value, direct_words))
        || (direct_words.starts_with(&["do", "not", "let", "anyone", "close"])
            && direct_close_scope(value, direct_words));
    let korean_actor = korean_close_actor_scope(value);
    let korean = value.contains("닫기")
        && [
            "넣지 마",
            "넣지마",
            "비활성화해",
            "사용하지 마",
            "사용하지마",
            "추가하지 마",
            "추가하지마",
            "꺼둬",
            "꺼 둬",
            "빼줘",
            "빼 줘",
        ]
        .iter()
        .any(|marker| value.contains(marker))
        && !korean_actor;
    english || korean
}

fn creator_close_directive(value: &str, words: &[&str], inherited_close_scope: bool) -> bool {
    let close_scope = direct_close_scope(value, words) || inherited_close_scope;
    let english = english_creator_close(words);
    let korean = ["만든 사람만", "방 생성자만", "방장만"]
        .iter()
        .any(|marker| value.contains(marker))
        && korean_close_permission(value);
    close_scope && (english || korean)
}

fn english_creator_close(words: &[&str]) -> bool {
    if contains_sequence(
        words,
        &[
            "close", "button", "must", "work", "only", "for", "the", "person", "who", "created",
        ],
    ) || (has_any(words, &["creator-only"])
        && has_any(words, &["close", "closing"])
        && has_any(words, &["allow", "make", "require"]))
        || (contains_sequence(words, &["only", "by", "the", "room", "creator"])
            && has_close_control(words)
            && has_any(words, &["can", "may", "must", "should"]))
    {
        return true;
    }
    let Some((start, end)) = english_creator_scope(words) else {
        return false;
    };
    let before = &words[..start];
    let after = &words[end..];
    let direct_permission = start == 0
        && (matches!(
            after,
            ["can" | "may" | "must" | "should", "close", ..]
                | ["should", "be", "able", "to", "close", ..]
                | ["is", "allowed", "to", "close", ..]
                | ["to", "close", ..]
        ) || (matches!(after, ["can" | "may" | "must" | "should", "use", ..])
            && has_close_control(after))
            || (after.starts_with(&["should", "be", "able", "to", "use"])
                && has_close_control(after))
            || (after.starts_with(&["is", "allowed", "to", "use"]) && has_close_control(after)));
    let let_permission = before.first() == Some(&"let")
        && (after.starts_with(&["close"])
            || (after.starts_with(&["use"]) && has_close_control(after)));
    let allow_permission = before.first() == Some(&"allow")
        && (after.starts_with(&["to", "close"])
            || (after.starts_with(&["to", "use"]) && has_close_control(after)));
    direct_permission || let_permission || allow_permission
}

fn english_creator_scope(words: &[&str]) -> Option<(usize, usize)> {
    for (index, window) in words.windows(7).enumerate() {
        if window == ["the", "room", "creator", "and", "no", "one", "else"] {
            return Some((index, index.saturating_add(7)));
        }
    }
    for (index, window) in words.windows(4).enumerate() {
        if window == ["the", "room", "creator", "alone"] {
            return Some((index, index.saturating_add(4)));
        }
    }
    for (index, window) in words.windows(7).enumerate() {
        if window == ["only", "the", "person", "who", "created", "the", "room"] {
            return Some((index, index.saturating_add(7)));
        }
    }
    for (index, window) in words.windows(6).enumerate() {
        if window == ["only", "person", "who", "created", "the", "room"] {
            return Some((index, index.saturating_add(6)));
        }
    }
    for (index, window) in words.windows(4).enumerate() {
        if window == ["only", "the", "room", "creator"] {
            return Some((index, index.saturating_add(4)));
        }
    }
    for (index, window) in words.windows(3).enumerate() {
        if window == ["only", "room", "creator"] || window == ["only", "the", "creator"] {
            return Some((index, index.saturating_add(3)));
        }
    }
    for (index, window) in words.windows(2).enumerate() {
        if window == ["only", "creator"] {
            return Some((index, index.saturating_add(2)));
        }
    }
    None
}

fn any_member_close_directive(value: &str, words: &[&str], inherited_close_scope: bool) -> bool {
    let close_scope = direct_close_scope(value, words) || inherited_close_scope;
    let english = english_any_member_close(words);
    let korean = ["모든 방 참가자", "모든 참가자", "모든 멤버", "누구나"]
        .iter()
        .any(|marker| value.contains(marker))
        && korean_close_permission(value);
    close_scope && (english || korean)
}

fn english_any_member_close(words: &[&str]) -> bool {
    let Some((start, end)) = english_member_scope(words) else {
        return false;
    };
    let before = &words[..start];
    let after = &words[end..];
    let direct_permission = start == 0
        && (matches!(
            after,
            ["can" | "may" | "must" | "should", "close", ..]
                | ["should", "be", "able", "to", "close", ..]
                | ["is", "allowed", "to", "close", ..]
        ) || (matches!(after, ["can" | "may" | "must" | "should", "use", ..])
            && has_close_control(after))
            || (after.starts_with(&["should", "be", "able", "to", "use"])
                && has_close_control(after)));
    let passive_permission = before.starts_with(&["the", "close", "button"])
        && before.ends_with(&["be", "used", "by"])
        && has_any(before, &["can", "may", "must", "should"]);
    let let_permission = before.first() == Some(&"let")
        && (after.starts_with(&["close"])
            || (after.starts_with(&["use"]) && has_close_control(after)));
    let allow_permission = before.first() == Some(&"allow")
        && (after.starts_with(&["to", "close"])
            || (after.starts_with(&["to", "use"]) && has_close_control(after)));
    let enabled_control = before.first() == Some(&"enable")
        && before.last() == Some(&"for")
        && has_any(before, &["close", "closing"]);
    let working_control =
        before.ends_with(&["work", "for"]) && has_any(before, &["close", "closing"]);
    direct_permission
        || passive_permission
        || let_permission
        || allow_permission
        || enabled_control
        || working_control
}

fn english_member_scope(words: &[&str]) -> Option<(usize, usize)> {
    if let Some(index) = words.iter().position(|word| *word == "anyone") {
        return Some((index, index.saturating_add(1)));
    }
    for (index, window) in words.windows(3).enumerate() {
        if matches!(
            window,
            ["any" | "all" | "every", "room", "member" | "members"]
        ) {
            return Some((index, index.saturating_add(3)));
        }
    }
    for (index, window) in words.windows(2).enumerate() {
        if matches!(window, ["any" | "all" | "every", "member" | "members"]) {
            return Some((index, index.saturating_add(2)));
        }
    }
    None
}

fn korean_close_permission(value: &str) -> bool {
    [
        "닫게 해",
        "닫게해",
        "닫을 수 있게 해",
        "닫을 수 있게해",
        "닫을 수 있어야 해",
        "닫을 수 있어",
        "닫아도 돼",
        "닫아도 된다",
        "닫기 버튼을 사용할 수 있게 해",
        "닫기 버튼을 사용할 수 있게해",
        "닫기 버튼을 사용할 수 있어야 해",
        "닫기 버튼 사용을 허용",
        "닫기 버튼을 사용하게 해",
        "닫기 버튼을 사용하게해",
        "방 닫기를 허용",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn korean_close_actor_scope(value: &str) -> bool {
    [
        "만든 사람",
        "모든 방 참가자",
        "모든 참가자",
        "모든 멤버",
        "누구나",
        "방 생성자",
        "방장",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

pub(super) fn direct_close_scope(value: &str, words: &[&str]) -> bool {
    let close_control = has_close_control(words);
    let direct_control_permission = contains_sequence(words, &["enable", "close", "for"])
        || contains_sequence(words, &["enable", "closing", "for"]);
    let close_room = [
        &["close", "room"][..],
        &["close", "a", "room"],
        &["close", "the", "room"],
        &["close", "that", "room"],
        &["close", "this", "room"],
        &["room", "closing"],
    ]
    .iter()
    .any(|sequence| contains_sequence(words, sequence));
    let room_actor = contains_sequence(words, &["room", "creator"])
        || contains_sequence(words, &["room", "member"])
        || contains_sequence(words, &["person", "who", "created"])
        || english_creator_scope(words).is_some()
        || english_member_scope(words).is_some();
    let implicit_room = room_actor
        && (contains_sequence(words, &["close", "it"])
            || words.last().is_some_and(|word| *word == "close"));
    let english = close_control || direct_control_permission || close_room || implicit_room;
    let korean_close = value.contains("닫기")
        || value.contains("닫아")
        || value.contains("닫을")
        || value.contains("닫는");
    let korean_business_target = ["메시지", "게시물", "스레드", "이슈", "티켓"]
        .iter()
        .any(|target| value.contains(target));
    let korean_room_target = ["방 닫", "방닫", "방을 닫"]
        .iter()
        .any(|target| value.contains(target));
    let korean = korean_close
        && (korean_room_target
            || value.contains("닫기 버튼")
            || value.contains("닫기 기능")
            || value.contains("닫기 컨트롤")
            || (contains_korean_room_token(value) && !korean_business_target));
    english || korean
}

fn contains_korean_room_token(value: &str) -> bool {
    value.match_indices('방').any(|(start, _)| {
        let suffix = value
            .get(start.saturating_add('방'.len_utf8())..)
            .unwrap_or_default();
        suffix.is_empty()
            || suffix
                .chars()
                .next()
                .is_some_and(|character| !character.is_alphanumeric())
            || KOREAN_TARGET_PARTICLES
                .iter()
                .any(|particle| suffix.starts_with(particle))
    })
}

fn has_close_control(words: &[&str]) -> bool {
    words.windows(2).any(|window| {
        matches!(
            window,
            ["close" | "closing", "button" | "control" | "feature"]
        )
    })
}

fn negative_close_scope(value: &str, words: &[&str]) -> bool {
    has_any(words, &["disabled", "never", "without"])
        || contains_sequence(words, &["do", "not"])
        || ["can", "may", "must", "should"]
            .iter()
            .any(|modal| contains_sequence(words, &[*modal, "not"]))
        || has_any(words, &["cannot"])
        || has_any(words, &["don't", "don’t", "dont"])
        || [
            "금지",
            "닫지 못",
            "닫을 수 없",
            "사용하지",
            "사용 못",
            "사용할 수 없",
        ]
        .iter()
        .any(|marker| value.contains(marker))
}

pub(super) fn unsupported_close_request(value: &str, words: &[&str]) -> bool {
    let business_target = has_any(
        words,
        &[
            "issue", "issues", "message", "messages", "post", "posts", "thread", "threads",
            "ticket", "tickets",
        ],
    ) || ["메시지", "게시물", "스레드", "이슈", "티켓"]
        .iter()
        .any(|target| value.contains(target));
    if business_target && !direct_close_scope(value, words) {
        return false;
    }
    if korean_close_non_normative(value) {
        return false;
    }
    let close_axis = direct_close_scope(value, words)
        || has_any(words, &["close", "closing"])
        || value.contains("닫기")
        || value.contains("닫을");
    let direct_words = strip_directive_prefixes(words);
    let direct_disable = direct_words
        .first()
        .is_some_and(|word| matches!(*word, "disable" | "enable" | "omit" | "remove"))
        && direct_close_scope(value, direct_words);
    let direct_creator_only = direct_words
        .first()
        .is_some_and(|word| matches!(*word, "make" | "require"))
        && has_any(direct_words, &["creator-only"])
        && has_close_control(direct_words);
    let direct_policy = direct_disable || direct_creator_only;
    let policy_language = direct_policy
        || unresolved_creator_close_policy(words)
        || unresolved_any_member_close_policy(words)
        || (unknown_close_actor(value, words) && normative_close_permission(words))
        || (english_member_scope(words).is_some()
            && negative_close_scope(value, words)
            && direct_close_scope(value, words))
        || korean_close_permission(value)
        || (korean_close_actor_scope(value) && negative_close_scope(value, words));
    close_axis && policy_language
}

fn korean_close_non_normative(value: &str) -> bool {
    [
        "감지",
        "분류",
        "알림을 보내",
        "없을 때",
        "있는지",
        "있을 때",
        "탐지",
        "확인",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

fn unresolved_creator_close_policy(words: &[&str]) -> bool {
    let creator_end = english_creator_scope(words)
        .map(|(_, end)| end)
        .or_else(|| {
            words.windows(2).enumerate().find_map(|(index, window)| {
                (window == ["room", "creator"]).then_some(index.saturating_add(2))
            })
        });
    let Some(creator_end) = creator_end else {
        return false;
    };
    let after = &words[creator_end..];
    matches!(
        after,
        ["can" | "may" | "must" | "should", "close", ..]
            | ["can" | "may" | "must" | "should", "not", "close", ..]
            | ["can" | "may" | "must" | "should", "not", "use", ..]
    ) || (matches!(after, ["can" | "may" | "must" | "should", "use", ..])
        && has_close_control(after))
}

fn unresolved_any_member_close_policy(words: &[&str]) -> bool {
    let Some((_, member_end)) = english_member_scope(words) else {
        return false;
    };
    normative_close_permission(&words[member_end..]) && direct_close_scope("", words)
}

fn normative_close_permission(words: &[&str]) -> bool {
    has_any(words, &["allow", "enable", "let"])
        || words.iter().enumerate().any(|(index, word)| {
            matches!(*word, "can" | "may" | "must" | "should")
                && words.get(index.saturating_add(1)).is_some_and(|next| {
                    matches!(*next, "close" | "use")
                        || (*next == "not"
                            && words
                                .get(index.saturating_add(2))
                                .is_some_and(|tail| matches!(*tail, "close" | "use")))
                })
        })
        || contains_sequence(words, &["is", "allowed", "to", "close"])
        || contains_sequence(words, &["is", "allowed", "to", "use"])
}

pub(super) fn inline_close_alternative(value: &str, words: &[&str]) -> bool {
    if (!has_alternative_connector(value, words) && !value.contains('/'))
        || !direct_close_scope(value, words)
    {
        return false;
    }
    let disabled = disabled_close_directive(value, words)
        || (direct_close_scope(value, words) && has_any(words, &["disabled"]));
    let any_member = has_any_member_actor(value, words);
    let creator_only = has_creator_actor(value, words);
    let unsupported_actor = unknown_close_actor(value, words);
    [disabled, any_member, creator_only, unsupported_actor]
        .into_iter()
        .filter(|branch| *branch)
        .count()
        > 1
}

fn unknown_close_actor(value: &str, words: &[&str]) -> bool {
    has_any(
        words,
        &[
            "admin",
            "admins",
            "administrator",
            "administrators",
            "guest",
            "guests",
            "host",
            "hosts",
            "moderator",
            "moderators",
            "owner",
            "owners",
            "role",
            "roles",
            "subscriber",
            "subscribers",
            "user",
            "users",
        ],
    ) || [
        "게스트",
        "관리자",
        "구독자",
        "운영자",
        "특정 역할",
        "호스트",
    ]
    .iter()
    .any(|actor| value.contains(actor))
}

pub(super) fn unsupported_close_alternative_branch(value: &str, words: &[&str]) -> bool {
    has_any(
        words,
        &[
            "admin",
            "admins",
            "administrator",
            "administrators",
            "creator",
            "guest",
            "guests",
            "host",
            "hosts",
            "moderator",
            "moderators",
            "owner",
            "owners",
            "role",
            "roles",
            "user",
            "users",
        ],
    ) || has_any(words, &["close", "closing", "permission", "permissions"])
        || korean_close_actor_scope(value)
}

pub(super) fn unsupported_close_modifier(value: &str, words: &[&str]) -> bool {
    let restriction = has_any(
        words,
        &[
            "approval",
            "after",
            "before",
            "confirmation",
            "during",
            "except",
            "excluding",
            "if",
            "locked",
            "unless",
            "until",
            "when",
            "whenever",
            "while",
        ],
    ) || contains_sequence(words, &["subject", "to"])
        || value.contains("승인")
        || value.contains("제외")
        || value.contains("확인 후");
    let scoped_target = words.windows(2).any(|window| {
        matches!(window[0], "at" | "on")
            && matches!(
                window[1],
                "night" | "weekdays" | "weekends" | "working-hours"
            )
    });
    direct_close_scope(value, words)
        && (restriction || scoped_target)
        && (normative_close_permission(words) || korean_close_permission(value))
}

pub(super) fn unsupported_connected_close_modifier(value: &str, words: &[&str]) -> bool {
    unknown_close_actor(value, words)
        && (normative_close_permission(words)
            || has_any(words, &["deny", "disable", "exclude", "forbid", "remove"]))
}

pub(super) fn connected_close_restriction(
    value: &str,
    directive_words: &[&str],
    continuation: Option<&str>,
) -> bool {
    let explicit_prefix = directive_words.starts_with(&["except"])
        || directive_words.starts_with(&["excluding"])
        || directive_words.starts_with(&["unless"])
        || value.starts_with("단 ");
    let elliptical_negative = unknown_close_actor(value, directive_words)
        && matches!(
            directive_words,
            [_, "cannot"] | ["but", _, "cannot"] | [_, "may", "not"] | ["but", _, "may", "not"]
        );
    let actor_exclusion =
        matches!(
            directive_words,
            [
                "except" | "excluding",
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "host"
                    | "hosts"
                    | "moderator"
                    | "moderators"
                    | "owner"
                    | "owners"
                    | "subscriber"
                    | "subscribers"
                    | "user"
                    | "users"
            ] | [
                "except" | "excluding",
                "the",
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "host"
                    | "hosts"
                    | "moderator"
                    | "moderators"
                    | "owner"
                    | "owners"
                    | "subscriber"
                    | "subscribers"
                    | "user"
                    | "users"
            ]
        ) || value.starts_with("단 ") && value.contains("제외") && directive_words.len() <= 4;
    let actor_exclusion_continues_close = continuation.is_none_or(|continuation| {
        let continuation_words = words(continuation);
        direct_close_scope(continuation, &continuation_words)
            || has_any(
                &continuation_words,
                &["approval", "cannot", "confirmation", "locked", "unless"],
            )
            || continuation.contains("승인")
            || continuation.contains("확인 후")
    });
    elliptical_negative
        || explicit_prefix
            && (actor_exclusion && actor_exclusion_continues_close
                || has_any(
                    directive_words,
                    &["approval", "cannot", "confirmation", "locked", "unless"],
                )
                || value.contains("승인")
                || value.contains("확인 후"))
}

pub(super) fn unsupported_close_condition_continuation(value: &str) -> bool {
    let words = words(value);
    has_any(
        &words,
        &[
            "active",
            "approval",
            "archived",
            "confirmation",
            "ends",
            "locked",
            "scheduled",
            "weekdays",
            "weekends",
        ],
    ) || value.contains("승인")
        || value.contains("잠겨")
        || value.contains("확인")
}
