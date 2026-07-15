use super::super::intent_interpretation::IntentBoundaryRequestV2;
use super::syntax::{self, BoundaryUnit, TextSpan, UnitLink};

pub(super) const GATE_TARGETS: &[&str] = &[
    "validation",
    "validator",
    "preview",
    "approval",
    "safety gate",
    "safety gates",
    "safety check",
    "safety checks",
    "safeguard",
    "safeguards",
    "검증",
    "미리보기",
    "승인",
    "안전 게이트",
    "안전게이트",
    "안전 장치",
    "안전장치",
    "보호 장치",
    "보호장치",
];
pub(super) const GATE_ACTIONS: &[&str] = &[
    "bypass",
    "bypassing",
    "skip",
    "skipping",
    "omit",
    "omitting",
    "disable",
    "disabling",
    "turn off",
    "remove",
    "removing",
    "ignore",
    "ignoring",
    "without",
    "우회",
    "건너뛰",
    "건너뛴",
    "생략",
    "무시",
    "끄",
    "제거",
    "없이",
    "거치지",
];
pub(super) const LIVE_ACTIONS: &[&str] = &[
    "deploy",
    "deploying",
    "apply",
    "applying",
    "execute",
    "executing",
    "run",
    "running",
    "connect",
    "connecting",
    "touch",
    "touching",
    "create",
    "creating",
    "delete",
    "deleting",
    "modify",
    "modifying",
    "change",
    "changing",
    "publish",
    "publishing",
    "post",
    "posting",
    "배포",
    "적용",
    "실행",
    "연결",
    "건드",
    "생성",
    "삭제",
    "수정",
    "변경",
    "게시",
];
pub(super) const LIVE_CONTEXT: &[&str] = &[
    "live discord",
    "live server",
    "production discord",
    "production server",
    "actual discord",
    "actual server",
    "discord now",
    "실서버",
    "라이브 디스코드",
    "라이브 서버",
    "운영 디스코드",
    "운영 서버",
    "운영서버",
    "실제 디스코드",
    "실제 서버",
];
pub(super) const IMMEDIATE_CONTEXT: &[&str] = &[
    "now",
    "immediately",
    "right away",
    "directly",
    "at once",
    "지금",
    "즉시",
    "바로",
    "직접",
    "당장",
    "실제로",
];
pub(super) const SECRET_TARGETS: &[&str] = &[
    "secret",
    "api key",
    "api token",
    "access key",
    "access token",
    "token",
    "credential",
    "password",
    "environment variable",
    "env var",
    "비밀",
    "api 키",
    "액세스 키",
    "토큰",
    "자격 증명",
    "자격증명",
    "비밀번호",
    "암호",
    "환경 변수",
    "환경변수",
    "키 값",
];
pub(super) const SECRET_ACTIONS: &[&str] = &[
    "expose",
    "exposing",
    "reveal",
    "revealing",
    "disclose",
    "disclosing",
    "publish",
    "publishing",
    "post",
    "posting",
    "show",
    "showing",
    "display",
    "displaying",
    "print",
    "printing",
    "include",
    "including",
    "put",
    "send",
    "sending",
    "leak",
    "leaking",
    "공개",
    "노출",
    "게시",
    "보여",
    "표시",
    "출력",
    "전송",
    "넣",
    "포함",
    "유출",
];
const SAFE_REDACTION: &[&str] = &[
    "redact",
    "redacted",
    "mask",
    "masked",
    "substitute",
    "substituted",
    "replace",
    "replaced",
    "가림",
    "가려",
    "마스킹",
    "대체",
    "치환",
    "숨김",
    "숨겨",
];
pub(super) const UNPROTECTED_SECRET: &[&str] = &[
    "without redaction",
    "without masking",
    "unredacted",
    "unmasked",
    "do not redact",
    "don't redact",
    "dont redact",
    "never redact",
    "do not mask",
    "don't mask",
    "dont mask",
    "가리지 말",
    "가림 없이",
    "마스킹 없이",
    "대체하지 말",
    "숨기지 말",
];
pub(super) const SECRET_DELIVERY_CONTEXT: &[&str] = &[
    "public",
    "everyone",
    "panel",
    "message",
    "response",
    "channel",
    "공개",
    "모두",
    "패널",
    "메시지",
    "응답",
    "채널",
];
pub(super) const PREFIX_NEGATIONS: &[&str] = &[
    "do not ",
    "don't ",
    "dont ",
    "never ",
    "must not ",
    "mustn't ",
    "should not ",
    "shouldn't ",
    "avoid ",
    "without ",
    "refuse to ",
    "no ",
    "안 ",
    "못 ",
    "절대 ",
    "금지 ",
];
pub(super) const SUFFIX_NEGATIONS: &[&str] = &[
    "not",
    "n't",
    "is forbidden",
    "is prohibited",
    "is disabled",
    "하지",
    "하지마",
    "하지 마",
    "하지 않",
    "지 않",
    "지마",
    "지 마",
    "지 말",
    "않아",
    "않고",
    "말아",
    "말고",
    "금지",
    "안 해",
    "안 함",
    "못 해",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoundaryKind {
    Gate,
    Live,
    Secret,
}

impl BoundaryKind {
    pub(super) fn request(self) -> IntentBoundaryRequestV2 {
        match self {
            Self::Gate => IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            Self::Live => IntentBoundaryRequestV2::DirectLiveMutation,
            Self::Secret => IntentBoundaryRequestV2::SecretDisclosure,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct UnitFacts {
    pub(super) gate_action: bool,
    pub(super) gate_target: bool,
    pub(super) live_action: bool,
    pub(super) live_strong_context: bool,
    pub(super) live_weak_context: bool,
    pub(super) immediate: bool,
    pub(super) secret_action: bool,
    pub(super) secret_target: bool,
    pub(super) secret_delivery: bool,
    pub(super) secret_unprotected: bool,
    pub(super) secret_safe: bool,
}

impl UnitFacts {
    pub(super) fn for_text(value: &str) -> Self {
        Self {
            gate_action: has_unnegated_marker(value, GATE_ACTIONS),
            gate_target: contains_any(value, GATE_TARGETS),
            live_action: has_unnegated_marker(value, LIVE_ACTIONS),
            live_strong_context: contains_any(value, LIVE_CONTEXT)
                || contains_any(value, &["live changes", "라이브 변경"]),
            live_weak_context: contains_any(value, live_weak_context()),
            immediate: contains_any(value, IMMEDIATE_CONTEXT),
            secret_action: has_unnegated_marker(value, SECRET_ACTIONS),
            secret_target: contains_any(value, SECRET_TARGETS),
            secret_delivery: contains_any(value, SECRET_DELIVERY_CONTEXT),
            secret_unprotected: contains_any(value, UNPROTECTED_SECRET),
            secret_safe: contains_any(value, SAFE_REDACTION),
        }
    }

    pub(super) fn for_unit(unit: &BoundaryUnit) -> Self {
        let mut facts = Self::for_text(&unit.text);
        if unit.inherited_action_negation {
            facts.gate_action = false;
            facts.live_action = false;
            facts.secret_action = false;
        }
        facts
    }

    pub(super) fn is_seed(&self, kind: BoundaryKind) -> bool {
        match kind {
            BoundaryKind::Gate => self.gate_action && self.gate_target,
            BoundaryKind::Live => self.live_action && (self.live_strong_context || self.immediate),
            BoundaryKind::Secret => {
                self.secret_target
                    && ((self.secret_action && (!self.secret_safe || self.secret_unprotected))
                        || (self.secret_unprotected && self.secret_delivery))
            }
        }
    }

    pub(super) fn has_evidence(&self, kind: BoundaryKind) -> bool {
        match kind {
            BoundaryKind::Gate => self.gate_action || self.gate_target,
            BoundaryKind::Live => {
                self.live_action
                    || self.live_strong_context
                    || self.live_weak_context
                    || self.immediate
            }
            BoundaryKind::Secret => {
                self.secret_action
                    || self.secret_target
                    || self.secret_delivery
                    || self.secret_unprotected
            }
        }
    }
}

pub(super) fn classify_sentence_units(
    visible: &[char],
    sentence: TextSpan,
    question: bool,
) -> Vec<BoundaryUnit> {
    let mut units = syntax::sentence_units(visible, sentence);
    apply_hypothetical_scope(&mut units, question);
    apply_coordinated_negation_scope(&mut units);
    units
}

fn apply_hypothetical_scope(units: &mut [BoundaryUnit], question: bool) {
    let mut inherited = question;
    for unit in units {
        let explicit_hypothetical = contains_hypothetical_marker(&unit.text);
        let explicit_assertion = contains_polite_request(&unit.text);
        if explicit_hypothetical {
            inherited = true;
        } else if explicit_assertion {
            inherited = false;
        }
        unit.hypothetical = inherited;
    }
}

fn apply_coordinated_negation_scope(units: &mut [BoundaryUnit]) {
    let mut inherited = false;
    for unit in units {
        if !matches!(unit.link, UnitLink::Additive | UnitLink::Alternative) {
            inherited = false;
        }
        unit.inherited_action_negation = inherited;
        if has_negated_action_marker(&unit.text) {
            inherited = true;
        } else if !unit.inherited_action_negation {
            inherited = false;
        }
    }
}

pub(super) fn live_weak_context() -> &'static [&'static str] {
    &[
        "discord",
        "server",
        "production",
        "actual",
        "디스코드",
        "서버",
        "운영",
        "실제",
    ]
}

pub(super) fn contains_any(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
}

fn contains_hypothetical_marker(value: &str) -> bool {
    const HYPOTHETICAL_MARKERS: &[&str] = &[
        "what if",
        "what happens if",
        "suppose that",
        "assuming that",
        "hypothetically",
        "in a hypothetical",
        "would it",
        "could someone",
        "can someone",
        "is it possible",
        "explain why",
        "explain how",
        "discuss whether",
        "tell me what would",
        "만약",
        "가정하면",
        "가정해서",
        "하면 어떻게",
        "하면 무슨",
        "되는지 설명",
        "가능한지",
        "가능 여부",
        "왜 우회",
    ];
    contains_any(value, HYPOTHETICAL_MARKERS)
}

fn contains_polite_request(value: &str) -> bool {
    const POLITE_REQUEST_MARKERS: &[&str] = &[
        "can you ",
        "could you ",
        "would you ",
        "will you ",
        "please ",
        "i want you to ",
        "i need you to ",
        "해줘",
        "해주세요",
        "해 줄래",
        "해줄래",
        "해주시",
        "부탁해",
    ];
    contains_any(value, POLITE_REQUEST_MARKERS)
}

fn has_unnegated_marker(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            marker_has_boundaries(value, start, start + matched.len())
                && !marker_is_negated(value, start, start + matched.len())
        })
    })
}

fn has_negated_action_marker(value: &str) -> bool {
    [GATE_ACTIONS, LIVE_ACTIONS, SECRET_ACTIONS]
        .into_iter()
        .flatten()
        .any(|marker| {
            value.match_indices(marker).any(|(start, matched)| {
                marker_has_boundaries(value, start, start + matched.len())
                    && marker_is_negated(value, start, start + matched.len())
            })
        })
}

fn marker_has_boundaries(value: &str, start: usize, end: usize) -> bool {
    let marker = &value[start..end];
    let left = value[..start].chars().next_back();
    let right = value[end..].chars().next();
    let left_valid = !marker
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        || !left.is_some_and(|character| character.is_ascii_alphanumeric());
    let right_valid = !marker
        .chars()
        .next_back()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        || !right.is_some_and(|character| character.is_ascii_alphanumeric());
    left_valid && right_valid
}

fn marker_is_negated(value: &str, start: usize, end: usize) -> bool {
    let prefix = preceding_chars(value, start, 48);
    let suffix = following_chars(value, end, 32);
    contains_any(&prefix, PREFIX_NEGATIONS)
        || SUFFIX_NEGATIONS
            .iter()
            .any(|negation| suffix.trim_start().starts_with(negation))
}

fn preceding_chars(value: &str, end: usize, limit: usize) -> String {
    value[..end]
        .chars()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn following_chars(value: &str, start: usize, limit: usize) -> String {
    value[start..].chars().take(limit).collect()
}
