use std::collections::BTreeSet;

use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

use super::intent_interpretation::IntentBoundaryRequestV2;

const GATE_TARGETS: &[&str] = &[
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
const GATE_ACTIONS: &[&str] = &[
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
const LIVE_ACTIONS: &[&str] = &[
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
const LIVE_CONTEXT: &[&str] = &[
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
const IMMEDIATE_CONTEXT: &[&str] = &[
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
const SECRET_TARGETS: &[&str] = &[
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
const SECRET_ACTIONS: &[&str] = &[
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
const UNPROTECTED_SECRET: &[&str] = &[
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
const SECRET_DELIVERY_CONTEXT: &[&str] = &[
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
const BOUNDARY_UNIT_CONNECTORS: &[&str] = &[
    " in order to ",
    " because ",
    " and then ",
    " but then ",
    " so that ",
    " however ",
    " instead ",
    " before ",
    " after ",
    " while ",
    " when ",
    " until ",
    " unless ",
    " yet ",
    " then ",
    " and ",
    " but ",
    " or ",
    "하기 전에 ",
    "한 후에 ",
    "하는 동안 ",
    "할 때 ",
    "하도록 ",
    " 그리고 ",
    " 하지만 ",
    " 그러나 ",
    " 대신 ",
    " 다음 ",
    "한 다음 ",
    "한 뒤 ",
    "하면서 ",
    "하고 ",
    "하며 ",
];
const PREFIX_NEGATIONS: &[&str] = &[
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
const SUFFIX_NEGATIONS: &[&str] = &[
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
struct TextSpan {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoundaryKind {
    Gate,
    Live,
    Secret,
}

impl BoundaryKind {
    fn request(self) -> IntentBoundaryRequestV2 {
        match self {
            Self::Gate => IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            Self::Live => IntentBoundaryRequestV2::DirectLiveMutation,
            Self::Secret => IntentBoundaryRequestV2::SecretDisclosure,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnitLink {
    Start,
    Additive,
    Alternative,
    Barrier,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundaryUnit {
    span: TextSpan,
    link: UnitLink,
    text: String,
    hypothetical: bool,
    inherited_action_negation: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct UnitFacts {
    gate_action: bool,
    gate_target: bool,
    live_action: bool,
    live_strong_context: bool,
    live_weak_context: bool,
    immediate: bool,
    secret_action: bool,
    secret_target: bool,
    secret_delivery: bool,
    secret_unprotected: bool,
    secret_safe: bool,
}

impl UnitFacts {
    fn for_text(value: &str) -> Self {
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

    fn for_unit(unit: &BoundaryUnit) -> Self {
        let mut facts = Self::for_text(&unit.text);
        if unit.inherited_action_negation {
            facts.gate_action = false;
            facts.live_action = false;
            facts.secret_action = false;
        }
        facts
    }

    fn is_seed(&self, kind: BoundaryKind) -> bool {
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

    fn has_evidence(&self, kind: BoundaryKind) -> bool {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundaryEvidenceGroup {
    kind: BoundaryKind,
    member_coverage_spans: Vec<TextSpan>,
    joiner_spans: Vec<TextSpan>,
    positive_role_spans: Vec<TextSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SafetyBoundaryAnalysis<'a> {
    source: &'a str,
    visible: Vec<char>,
    groups: Vec<BoundaryEvidenceGroup>,
    requests: Vec<IntentBoundaryRequestV2>,
    ownership_ambiguous: bool,
}

impl<'a> SafetyBoundaryAnalysis<'a> {
    pub(crate) fn analyze(human_message: &'a str) -> Self {
        let quote_mask = mask_quoted_text(human_message);
        let visible = quote_mask.visible;
        let mut groups = Vec::new();
        for (span, question) in sentence_spans(&visible) {
            let units = sentence_units(&visible, span, question);
            groups.extend(evidence_groups(&units, &visible));
        }
        let requests = groups
            .iter()
            .map(|group| group.kind.request())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            source: human_message,
            visible,
            groups,
            requests,
            ownership_ambiguous: quote_mask.unmatched,
        }
    }

    pub(crate) fn requests(&self) -> &[IntentBoundaryRequestV2] {
        &self.requests
    }

    pub(crate) fn owns_capability_evidence(&self, candidate: &str) -> bool {
        if self.ownership_ambiguous {
            return false;
        }
        let Some(candidate_span) =
            unique_visible_bounded_span(self.source, &self.visible, candidate)
        else {
            return false;
        };
        self.groups
            .iter()
            .any(|group| group_covers_candidate(group, candidate_span, &self.visible))
    }
}

pub(crate) fn analyze_safety_boundaries(human_message: &str) -> SafetyBoundaryAnalysis<'_> {
    SafetyBoundaryAnalysis::analyze(human_message)
}

pub(crate) fn ground_safety_boundary_requests(human_message: &str) -> Vec<IntentBoundaryRequestV2> {
    analyze_safety_boundaries(human_message).requests().to_vec()
}

pub(crate) fn safety_boundary_owns_capability_evidence(
    human_message: &str,
    candidate: &str,
) -> bool {
    analyze_safety_boundaries(human_message).owns_capability_evidence(candidate)
}

fn live_weak_context() -> &'static [&'static str] {
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

fn sentence_spans(visible: &[char]) -> Vec<(TextSpan, bool)> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    for (index, character) in visible.iter().enumerate() {
        if is_sentence_boundary(*character) {
            if let Some(span) = trimmed_span(visible, start, index) {
                spans.push((span, is_question_mark(*character)));
            }
            start = index.saturating_add(1);
        }
    }
    if let Some(span) = trimmed_span(visible, start, visible.len()) {
        spans.push((span, false));
    }
    spans
}

fn sentence_units(visible: &[char], sentence: TextSpan, question: bool) -> Vec<BoundaryUnit> {
    let mut units = Vec::new();
    let mut start = sentence.start;
    let mut link = UnitLink::Start;
    let mut index = sentence.start;
    while index < sentence.end {
        if matches!(visible[index], ',' | '，' | '、') {
            push_boundary_unit(&mut units, visible, start, index, link);
            start = index.saturating_add(1);
            link = UnitLink::Additive;
            index = start;
            continue;
        }
        if let Some((length, next_link)) = connector_at(visible, index, sentence.end) {
            push_boundary_unit(&mut units, visible, start, index, link);
            index = index.saturating_add(length);
            start = index;
            link = next_link;
            continue;
        }
        index = index.saturating_add(1);
    }
    push_boundary_unit(&mut units, visible, start, sentence.end, link);
    apply_hypothetical_scope(&mut units, question);
    apply_coordinated_negation_scope(&mut units);
    units
}

fn push_boundary_unit(
    units: &mut Vec<BoundaryUnit>,
    visible: &[char],
    start: usize,
    end: usize,
    link: UnitLink,
) {
    let Some(span) = trimmed_span(visible, start, end) else {
        return;
    };
    let text = normalized_text(
        &visible[span.start..span.end]
            .iter()
            .collect::<String>()
            .to_lowercase(),
    );
    units.push(BoundaryUnit {
        span,
        link,
        text,
        hypothetical: false,
        inherited_action_negation: false,
    });
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

fn trimmed_span(value: &[char], start: usize, end: usize) -> Option<TextSpan> {
    let start = (start..end).find(|index| !value[*index].is_whitespace())?;
    let end = (start..end)
        .rfind(|index| !value[*index].is_whitespace())?
        .saturating_add(1);
    Some(TextSpan { start, end })
}

fn connector_at(visible: &[char], start: usize, end: usize) -> Option<(usize, UnitLink)> {
    BOUNDARY_UNIT_CONNECTORS
        .iter()
        .filter_map(|connector| {
            let connector_length = connector.chars().count();
            let connector_end = start.saturating_add(connector_length);
            (connector_end <= end
                && ascii_case_insensitive_str_equal(&visible[start..connector_end], connector))
            .then_some((connector_length, connector_link(connector)))
        })
        .max_by_key(|(length, _)| *length)
}

fn connector_link(connector: &str) -> UnitLink {
    if connector == " or " {
        UnitLink::Alternative
    } else if matches!(
        connector,
        " and then "
            | " then "
            | " and "
            | " 그리고 "
            | " 다음 "
            | "한 다음 "
            | "한 뒤 "
            | "하고 "
            | "하며 "
    ) {
        UnitLink::Additive
    } else {
        UnitLink::Barrier
    }
}

fn ascii_case_insensitive_str_equal(left: &[char], right: &str) -> bool {
    left.iter().zip(right.chars()).all(|(left, right)| {
        *left == right || (left.is_ascii() && right.is_ascii() && left.eq_ignore_ascii_case(&right))
    })
}

fn ascii_case_insensitive_chars_equal(left: &[char], right: &[char]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left == right
                || (left.is_ascii() && right.is_ascii() && left.eq_ignore_ascii_case(right))
        })
}

fn evidence_groups(units: &[BoundaryUnit], visible: &[char]) -> Vec<BoundaryEvidenceGroup> {
    let facts = units
        .iter()
        .map(|unit| {
            if unit.hypothetical {
                UnitFacts::default()
            } else {
                UnitFacts::for_unit(unit)
            }
        })
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    for kind in [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret] {
        let mut intervals = BTreeSet::new();
        for (index, unit_facts) in facts.iter().enumerate() {
            if unit_facts.is_seed(kind) {
                intervals.insert(expanded_group_interval(units, &facts, kind, index, index));
            }
        }
        if kind == BoundaryKind::Live {
            for index in 0..units.len().saturating_sub(1) {
                if units[index].hypothetical
                    || units[index + 1].hypothetical
                    || units[index + 1].link == UnitLink::Alternative
                    || !unit_is_boundary_component_only(&units[index].text, kind)
                    || !unit_is_boundary_component_only(&units[index + 1].text, kind)
                {
                    continue;
                }
                let combined = combine_facts(&facts[index], &facts[index + 1]);
                if combined.is_seed(kind) {
                    intervals.insert(expanded_group_interval(
                        units,
                        &facts,
                        kind,
                        index,
                        index + 1,
                    ));
                }
            }
        }
        for (start, end) in intervals {
            groups.push(evidence_group_for_interval(
                units, visible, kind, start, end,
            ));
        }
    }
    groups
}

fn evidence_group_for_interval(
    units: &[BoundaryUnit],
    visible: &[char],
    kind: BoundaryKind,
    start: usize,
    end: usize,
) -> BoundaryEvidenceGroup {
    let mut member_coverage_spans = Vec::new();
    let mut joiner_spans = Vec::new();
    let mut positive_role_spans = Vec::new();
    for index in start..=end {
        if index > start && units[index].link == UnitLink::Additive {
            joiner_spans.push(TextSpan {
                start: units[index - 1].span.end,
                end: units[index].span.start,
            });
        }
        member_coverage_spans.extend(boundary_coverage_spans(visible, units[index].span, kind));
        positive_role_spans.extend(positive_role_spans_for_unit(visible, &units[index], kind));
    }
    BoundaryEvidenceGroup {
        kind,
        member_coverage_spans,
        joiner_spans,
        positive_role_spans,
    }
}

fn group_covers_candidate(
    group: &BoundaryEvidenceGroup,
    candidate: TextSpan,
    visible: &[char],
) -> bool {
    let mut has_content = false;
    for (offset, character) in visible[candidate.start..candidate.end].iter().enumerate() {
        if !word_continuation(*character) {
            continue;
        }
        has_content = true;
        let index = candidate.start.saturating_add(offset);
        if !group
            .member_coverage_spans
            .iter()
            .chain(&group.joiner_spans)
            .any(|span| index >= span.start && index < span.end)
        {
            return false;
        }
    }
    has_content
        && group
            .positive_role_spans
            .iter()
            .any(|span| candidate.start < span.end && candidate.end > span.start)
}

fn expanded_group_interval(
    units: &[BoundaryUnit],
    facts: &[UnitFacts],
    kind: BoundaryKind,
    mut start: usize,
    mut end: usize,
) -> (usize, usize) {
    while start > 0
        && units[start].link == UnitLink::Additive
        && !units[start - 1].hypothetical
        && facts[start - 1].has_evidence(kind)
        && unit_is_boundary_component_only(&units[start - 1].text, kind)
    {
        start -= 1;
    }
    while end + 1 < units.len()
        && units[end + 1].link == UnitLink::Additive
        && !units[end + 1].hypothetical
        && facts[end + 1].has_evidence(kind)
        && unit_is_boundary_component_only(&units[end + 1].text, kind)
    {
        end += 1;
    }
    (start, end)
}

fn combine_facts(left: &UnitFacts, right: &UnitFacts) -> UnitFacts {
    UnitFacts {
        gate_action: left.gate_action || right.gate_action,
        gate_target: left.gate_target || right.gate_target,
        live_action: left.live_action || right.live_action,
        live_strong_context: left.live_strong_context || right.live_strong_context,
        live_weak_context: left.live_weak_context || right.live_weak_context,
        immediate: left.immediate || right.immediate,
        secret_action: left.secret_action || right.secret_action,
        secret_target: left.secret_target || right.secret_target,
        secret_delivery: left.secret_delivery || right.secret_delivery,
        secret_unprotected: left.secret_unprotected || right.secret_unprotected,
        secret_safe: left.secret_safe || right.secret_safe,
    }
}

fn unique_visible_bounded_span(
    source: &str,
    visible: &[char],
    candidate: &str,
) -> Option<TextSpan> {
    if candidate.is_empty() {
        return None;
    }
    let candidate_length = candidate.chars().count();
    let mut occurrence = None;
    for (byte_start, _) in source.match_indices(candidate) {
        let byte_end = byte_start.saturating_add(candidate.len());
        if !bounded_string_occurrence(source, candidate, byte_start, byte_end) {
            continue;
        }
        let start = source[..byte_start].chars().count();
        let end = start.saturating_add(candidate_length);
        if !visible
            .get(start..end)
            .is_some_and(|value| value.iter().copied().eq(candidate.chars()))
        {
            continue;
        }
        if occurrence.is_some() {
            return None;
        }
        occurrence = Some(TextSpan { start, end });
    }
    occurrence
}

fn bounded_string_occurrence(source: &str, candidate: &str, start: usize, end: usize) -> bool {
    let left_valid = !candidate.chars().next().is_some_and(word_continuation)
        || !source[..start]
            .chars()
            .next_back()
            .is_some_and(word_continuation);
    let right_valid = !candidate.chars().next_back().is_some_and(word_continuation)
        || !source[end..].chars().next().is_some_and(word_continuation)
        || known_korean_suffix_boundary(&source[end..]);
    left_valid && right_valid
}

fn known_korean_suffix_boundary(value: &str) -> bool {
    ["해주세요", "해줘", "하도록", "하게", "하고", "하며"]
        .iter()
        .any(|suffix| value.starts_with(suffix))
}

fn unit_is_boundary_component_only(value: &str, kind: BoundaryKind) -> bool {
    let value = normalized_text(&value.to_lowercase());
    !value.is_empty()
        && UnitFacts::for_text(&value).has_evidence(kind)
        && boundary_text_is_covered(&value, kind)
}

fn boundary_text_is_covered(value: &str, kind: BoundaryKind) -> bool {
    let characters = value.chars().collect::<Vec<_>>();
    let mut covered = vec![false; characters.len()];
    for markers in boundary_candidate_markers(kind) {
        cover_markers(&characters, &mut covered, markers);
    }
    cover_markers(&characters, &mut covered, english_neutral_words());
    cover_markers(&characters, &mut covered, korean_neutral_fragments());
    characters
        .iter()
        .enumerate()
        .all(|(index, character)| covered[index] || !word_continuation(*character))
}

fn boundary_coverage_spans(visible: &[char], span: TextSpan, kind: BoundaryKind) -> Vec<TextSpan> {
    let characters = &visible[span.start..span.end];
    let mut covered = vec![false; characters.len()];
    for markers in boundary_candidate_markers(kind) {
        cover_markers(characters, &mut covered, markers);
    }
    cover_markers(characters, &mut covered, english_neutral_words());
    cover_markers(characters, &mut covered, korean_neutral_fragments());
    covered_runs(&covered, span.start)
}

fn covered_runs(covered: &[bool], offset: usize) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut start = None;
    for index in 0..=covered.len() {
        if covered.get(index).copied().unwrap_or(false) {
            start.get_or_insert(index);
        } else if let Some(run_start) = start.take() {
            spans.push(TextSpan {
                start: offset.saturating_add(run_start),
                end: offset.saturating_add(index),
            });
        }
    }
    spans
}

fn positive_role_spans_for_unit(
    visible: &[char],
    unit: &BoundaryUnit,
    kind: BoundaryKind,
) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    if !unit.inherited_action_negation {
        let actions = match kind {
            BoundaryKind::Gate => GATE_ACTIONS,
            BoundaryKind::Live => LIVE_ACTIONS,
            BoundaryKind::Secret => SECRET_ACTIONS,
        };
        spans.extend(marker_occurrence_spans(visible, unit.span, actions, true));
    }
    match kind {
        BoundaryKind::Gate => {
            spans.extend(marker_occurrence_spans(
                visible,
                unit.span,
                GATE_TARGETS,
                false,
            ));
        }
        BoundaryKind::Live => {
            for markers in [
                LIVE_CONTEXT,
                live_weak_context(),
                IMMEDIATE_CONTEXT,
                &["live changes", "라이브 변경"],
            ] {
                spans.extend(marker_occurrence_spans(visible, unit.span, markers, false));
            }
        }
        BoundaryKind::Secret => {
            for markers in [SECRET_TARGETS, UNPROTECTED_SECRET] {
                spans.extend(marker_occurrence_spans(visible, unit.span, markers, false));
            }
        }
    }
    spans
}

fn marker_occurrence_spans(
    visible: &[char],
    span: TextSpan,
    markers: &[&str],
    require_unnegated: bool,
) -> Vec<TextSpan> {
    let characters = &visible[span.start..span.end];
    let mut spans = Vec::new();
    for marker in markers {
        let marker = marker.chars().collect::<Vec<_>>();
        if marker.is_empty() || marker.len() > characters.len() {
            continue;
        }
        for start in 0..=characters.len().saturating_sub(marker.len()) {
            let end = start.saturating_add(marker.len());
            if ascii_case_insensitive_chars_equal(&characters[start..end], &marker)
                && char_marker_has_boundaries(characters, &marker, start, end)
                && (!require_unnegated || !char_marker_is_negated(characters, start, end))
            {
                spans.push(TextSpan {
                    start: span.start.saturating_add(start),
                    end: span.start.saturating_add(end),
                });
            }
        }
    }
    spans
}

fn char_marker_is_negated(value: &[char], start: usize, end: usize) -> bool {
    let prefix = value[start.saturating_sub(48)..start]
        .iter()
        .collect::<String>()
        .to_lowercase();
    let suffix = value[end..value.len().min(end.saturating_add(32))]
        .iter()
        .collect::<String>()
        .to_lowercase();
    contains_any(&prefix, PREFIX_NEGATIONS)
        || SUFFIX_NEGATIONS
            .iter()
            .any(|negation| suffix.trim_start().starts_with(negation))
}

fn boundary_candidate_markers(kind: BoundaryKind) -> Vec<&'static [&'static str]> {
    match kind {
        BoundaryKind::Gate => vec![GATE_ACTIONS, GATE_TARGETS],
        BoundaryKind::Live => vec![
            LIVE_ACTIONS,
            LIVE_CONTEXT,
            live_weak_context(),
            IMMEDIATE_CONTEXT,
            &["live changes", "라이브 변경"],
        ],
        BoundaryKind::Secret => vec![
            SECRET_ACTIONS,
            SECRET_TARGETS,
            UNPROTECTED_SECRET,
            SECRET_DELIVERY_CONTEXT,
        ],
    }
}

fn cover_markers(characters: &[char], covered: &mut [bool], markers: &[&str]) {
    for marker in markers {
        let marker = marker.chars().collect::<Vec<_>>();
        if marker.is_empty() || marker.len() > characters.len() {
            continue;
        }
        for start in 0..=characters.len().saturating_sub(marker.len()) {
            let end = start.saturating_add(marker.len());
            if ascii_case_insensitive_chars_equal(&characters[start..end], &marker)
                && char_marker_has_boundaries(characters, &marker, start, end)
            {
                covered[start..end].fill(true);
            }
        }
    }
}

fn char_marker_has_boundaries(value: &[char], marker: &[char], start: usize, end: usize) -> bool {
    let left_valid = !marker
        .first()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        || start == 0
        || !value[start - 1..start]
            .first()
            .is_some_and(|character| character.is_ascii_alphanumeric());
    let right_valid = !marker
        .last()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        || !value
            .get(end)
            .is_some_and(|character| character.is_ascii_alphanumeric());
    left_valid && right_valid
}

fn english_neutral_words() -> &'static [&'static str] {
    &[
        "a",
        "all",
        "an",
        "and",
        "any",
        "at",
        "but",
        "design",
        "directly",
        "every",
        "for",
        "from",
        "in",
        "immediately",
        "into",
        "it",
        "now",
        "of",
        "on",
        "only",
        "please",
        "the",
        "them",
        "then",
        "these",
        "this",
        "those",
        "to",
        "user",
        "users",
        "value",
        "values",
    ]
}

fn korean_neutral_fragments() -> &'static [&'static str] {
    &[
        "해주세요",
        "하도록",
        "하게",
        "해줘",
        "에서",
        "에게",
        "으로",
        "은",
        "는",
        "이",
        "가",
        "을",
        "를",
        "의",
        "에",
        "로",
        "와",
        "과",
        "만",
        "도",
        "고",
        "해",
    ]
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

#[derive(Clone, Copy)]
struct QuoteState {
    end: char,
    fence_len: usize,
    start: usize,
}

struct QuoteMask {
    visible: Vec<char>,
    unmatched: bool,
}

fn mask_quoted_text(value: &str) -> QuoteMask {
    let characters = value.chars().collect::<Vec<_>>();
    let mut masked = characters.clone();
    let mut quote: Option<QuoteState> = None;
    let mut index = 0usize;
    while index < characters.len() {
        if let Some(active) = quote {
            if active.end == '`' && characters[index] == '`' {
                let run = repeated_character_count(&characters, index, '`');
                if run >= active.fence_len {
                    for value in masked.iter_mut().skip(index).take(active.fence_len) {
                        *value = ' ';
                    }
                    index = index.saturating_add(active.fence_len);
                    quote = None;
                    continue;
                }
            } else if characters[index] == active.end
                && !is_escaped(&characters, index)
                && !is_inner_apostrophe(&characters, index)
            {
                masked[index] = ' ';
                index = index.saturating_add(1);
                quote = None;
                continue;
            }
            masked[index] = ' ';
            index = index.saturating_add(1);
            continue;
        }

        let Some((end, fence_len)) = opening_quote(&characters, index) else {
            index = index.saturating_add(1);
            continue;
        };
        if is_escaped(&characters, index) || is_inner_apostrophe(&characters, index) {
            index = index.saturating_add(1);
            continue;
        }
        let start = index;
        for value in masked.iter_mut().skip(index).take(fence_len) {
            *value = ' ';
        }
        index = index.saturating_add(fence_len);
        quote = Some(QuoteState {
            end,
            fence_len,
            start,
        });
    }
    let unmatched = quote.is_some();
    if let Some(active) = quote {
        masked[active.start..].copy_from_slice(&characters[active.start..]);
    }
    QuoteMask {
        visible: masked,
        unmatched,
    }
}

fn opening_quote(characters: &[char], index: usize) -> Option<(char, usize)> {
    match characters[index] {
        '"' => Some(('"', 1)),
        '\'' => Some(('\'', 1)),
        '`' => Some(('`', repeated_character_count(characters, index, '`'))),
        '“' => Some(('”', 1)),
        '‘' => Some(('’', 1)),
        '«' => Some(('»', 1)),
        '‹' => Some(('›', 1)),
        '〈' => Some(('〉', 1)),
        '《' => Some(('》', 1)),
        '「' => Some(('」', 1)),
        '『' => Some(('』', 1)),
        '【' => Some(('】', 1)),
        _ => None,
    }
}

fn repeated_character_count(characters: &[char], start: usize, expected: char) -> usize {
    characters[start..]
        .iter()
        .take_while(|character| **character == expected)
        .count()
}

fn is_escaped(characters: &[char], index: usize) -> bool {
    let preceding_slashes = characters[..index]
        .iter()
        .rev()
        .take_while(|character| **character == '\\')
        .count();
    preceding_slashes % 2 == 1
}

fn is_inner_apostrophe(characters: &[char], index: usize) -> bool {
    characters[index] == '\''
        && index > 0
        && index + 1 < characters.len()
        && characters[index - 1].is_alphanumeric()
        && characters[index + 1].is_alphanumeric()
}

fn is_sentence_boundary(character: char) -> bool {
    matches!(
        character,
        '.' | '!' | '?' | ';' | '\n' | '\r' | '。' | '！' | '？'
    )
}

fn is_question_mark(character: char) -> bool {
    matches!(character, '?' | '？')
}

fn normalized_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn contains_any(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
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

#[cfg(test)]
mod tests {
    use super::{
        ground_safety_boundary_requests, safety_boundary_owns_capability_evidence,
        IntentBoundaryRequestV2,
    };

    #[test]
    fn grounds_gate_bypass_and_direct_live_mutation() {
        assert_eq!(
            ground_safety_boundary_requests(
                "Please bypass validation, preview, and approval, then connect to the live Discord server now and create the roles directly."
            ),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            ]
        );
    }

    #[test]
    fn grounds_secret_disclosure_and_immediate_deployment() {
        assert_eq!(
            ground_safety_boundary_requests(
                "Immediately deploy to the production Discord server and publish the API token value in a public channel without masking it."
            ),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::SecretDisclosure,
            ]
        );
    }

    #[test]
    fn distinguishes_one_gate_bypass_from_retained_gates() {
        assert_eq!(
            ground_safety_boundary_requests(
                "Keep validation and preview, skip only approval, and apply the changes to live Discord right away."
            ),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            ]
        );
    }

    #[test]
    fn keeps_negated_live_mutation_out_of_an_all_gate_bypass() {
        assert_eq!(
            ground_safety_boundary_requests(
                "Prepare a static workflow, but omit every safety gate. Do not touch the live server or deploy anything."
            ),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
    }

    #[test]
    fn accepts_safe_redaction_and_explicit_non_deployment() {
        assert!(ground_safety_boundary_requests(
            "Create a panel whose message says credentials are masked and substituted with [HIDDEN]. Keep every safety check and do not deploy it or reveal any real token."
        )
        .is_empty());
    }

    #[test]
    fn ignores_quoted_and_code_formatted_interface_copy() {
        assert!(ground_safety_boundary_requests(
            "Create preview buttons labeled 'Skip approval', \"Reveal the API key\", and `Deploy to live Discord now`. Do not deploy the design."
        )
        .is_empty());
        assert!(ground_safety_boundary_requests(
            "Render ```bypass validation and expose the secret``` as documentation, without deploying it."
        )
        .is_empty());
    }

    #[test]
    fn treats_hypotheticals_as_non_commands() {
        assert!(ground_safety_boundary_requests(
            "What happens if someone bypasses approval and deploys to a live server?"
        )
        .is_empty());
        assert!(ground_safety_boundary_requests(
            "승인을 우회하고 실서버에 배포하면 어떻게 되나요?"
        )
        .is_empty());
    }

    #[test]
    fn treats_polite_questions_as_requests() {
        assert_eq!(
            ground_safety_boundary_requests(
                "Could you skip approval and deploy to live Discord now?"
            ),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            ]
        );
    }

    #[test]
    fn grounds_korean_gate_and_live_requests() {
        assert_eq!(
            ground_safety_boundary_requests(
                "검증과 미리보기는 유지하고 승인만 건너뛴 다음 지금 디스코드 실서버에 바로 적용해줘."
            ),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            ]
        );
    }

    #[test]
    fn grounds_korean_secret_and_live_requests() {
        assert_eq!(
            ground_safety_boundary_requests(
                "환경 변수의 API 키 값을 가리지 말고 공개 채널에 게시하고 운영 서버에 즉시 배포해줘."
            ),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::SecretDisclosure,
            ]
        );
    }

    #[test]
    fn keeps_contracted_negations_visible_and_safe() {
        assert!(ground_safety_boundary_requests(
            "Don't deploy or expose any secret, and don't skip approval."
        )
        .is_empty());
    }

    #[test]
    fn returns_canonical_deduplicated_order() {
        assert_eq!(
            ground_safety_boundary_requests(
                "Bypass approval and validation, skip approval again, expose the API key in public, and deploy to the live server now."
            ),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
                IntentBoundaryRequestV2::SecretDisclosure,
            ]
        );
    }

    #[test]
    fn ordinary_discord_design_language_is_not_live_mutation() {
        assert!(ground_safety_boundary_requests(
            "Build a Discord automation design with validation, preview, approval, and redacted secret placeholders."
        )
        .is_empty());
    }

    #[test]
    fn unmatched_quotes_do_not_hide_boundary_requests() {
        let human =
            "Use the label 'temporary and then bypass approval and deploy to the live server now";
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            ]
        );
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "bypass approval"
        ));
    }

    #[test]
    fn owns_exact_gate_and_live_boundary_fragments() {
        let human = "Skip only user approval, keep validation and preview, then connect to Discord now and deploy the live changes immediately.";
        for candidate in [
            "Skip only user approval",
            "connect to Discord",
            "deploy the live changes immediately",
        ] {
            assert!(safety_boundary_owns_capability_evidence(human, candidate));
        }
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "keep validation and preview"
        ));
    }

    #[test]
    fn preserves_unrelated_requirement_in_the_same_clause() {
        let human = "Post an audit record and deploy the live changes immediately.";
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "Post an audit record"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "deploy the live changes immediately"
        ));
    }

    #[test]
    fn owns_secret_disclosure_without_claiming_adjacent_behavior() {
        let human =
            "Post an audit record and expose the API token in a public channel without masking it.";
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "Post an audit record"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "expose the API token"
        ));
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "public channel"
        ));
    }

    #[test]
    fn mixed_boundary_and_capability_evidence_fails_safe() {
        let mixed = "Post an immutable audit record and deploy the live changes immediately.";
        assert!(!safety_boundary_owns_capability_evidence(
            mixed,
            "Post an immutable audit record and deploy the live changes immediately"
        ));
        let subordinated =
            "Post a production readiness report before deploying to live Discord now.";
        assert!(!safety_boundary_owns_capability_evidence(
            subordinated,
            "Post a production readiness report"
        ));
    }

    #[test]
    fn ambiguous_exact_candidate_fails_safe() {
        let human = "Deploy live Discord now. Deploy live Discord now.";
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "Deploy live Discord now"
        ));
    }

    #[test]
    fn quoted_hypothetical_and_negated_candidates_are_not_owned() {
        let quoted =
            "Use the label 'Deploy live Discord now', then deploy the live changes immediately.";
        assert!(!safety_boundary_owns_capability_evidence(
            quoted,
            "Deploy live Discord now"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            quoted,
            "deploy the live changes immediately"
        ));

        let hypothetical =
            "What happens if someone deploys to live Discord now? Deploy the live changes immediately.";
        assert!(!safety_boundary_owns_capability_evidence(
            hypothetical,
            "deploys to live Discord now"
        ));

        let negated = "Do not deploy live Discord now. Deploy the live changes immediately.";
        assert!(!safety_boundary_owns_capability_evidence(
            negated,
            "deploy live Discord now"
        ));
    }

    #[test]
    fn owns_korean_boundary_fragments_without_claiming_adjacent_behavior() {
        let human = "감사 기록을 게시하고 승인만 건너뛴 다음 운영 서버에 즉시 배포해줘.";
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "감사 기록을 게시"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "승인만 건너뛴"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "운영 서버에 즉시 배포"
        ));
    }

    #[test]
    fn relates_actions_to_their_own_targets() {
        let human = "Keep approval and skip the tutorial.";
        assert!(ground_safety_boundary_requests(human).is_empty());
        for candidate in ["approval", "skip the tutorial", human.trim_end_matches('.')] {
            assert!(!safety_boundary_owns_capability_evidence(human, candidate));
        }
    }

    #[test]
    fn scopes_negation_to_the_local_predicate() {
        let human = "Skip approval and do not omit validation.";
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
        for candidate in ["Skip approval", "approval"] {
            assert!(safety_boundary_owns_capability_evidence(human, candidate));
        }
        for candidate in ["do not omit validation", "omit validation", "validation"] {
            assert!(!safety_boundary_owns_capability_evidence(human, candidate));
        }

        let contrast = "Do not omit validation yet skip approval.";
        assert_eq!(
            ground_safety_boundary_requests(contrast),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
        assert!(safety_boundary_owns_capability_evidence(
            contrast,
            "skip approval"
        ));

        let korean = "승인을 건너뛰지 말고 검증을 유지해줘.";
        assert!(ground_safety_boundary_requests(korean).is_empty());
    }

    #[test]
    fn excludes_negated_alternative_from_owned_gate_evidence() {
        let human = "Skip approval or do not omit validation.";
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "Skip approval"
        ));
        for candidate in ["omit validation", "validation"] {
            assert!(!safety_boundary_owns_capability_evidence(human, candidate));
        }
    }

    #[test]
    fn owns_only_the_target_related_to_the_bypass_action() {
        let human = "Skip validation because approval remains mandatory.";
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
        for candidate in ["Skip validation", "validation"] {
            assert!(safety_boundary_owns_capability_evidence(human, candidate));
        }
        for candidate in ["approval", "approval remains mandatory"] {
            assert!(!safety_boundary_owns_capability_evidence(human, candidate));
        }
    }

    #[test]
    fn resumes_after_a_hypothetical_with_an_asserted_request() {
        let human = "What happens if someone skips approval, but please skip approval now.";
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "skips approval"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "skip approval now"
        ));
    }

    #[test]
    fn keeps_polite_hypothetical_explanations_non_executable() {
        let human =
            "Could you explain what happens if we skip approval and deploy live Discord now?";
        assert!(ground_safety_boundary_requests(human).is_empty());
        for candidate in ["skip approval", "deploy live Discord now"] {
            assert!(!safety_boundary_owns_capability_evidence(human, candidate));
        }
    }

    #[test]
    fn propagates_asserted_and_hypothetical_question_scopes() {
        let asserted = "Could you skip approval and deploy live Discord now?";
        assert_eq!(
            ground_safety_boundary_requests(asserted),
            vec![
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            ]
        );
        for candidate in ["skip approval", "deploy live Discord now"] {
            assert!(safety_boundary_owns_capability_evidence(
                asserted, candidate
            ));
        }

        let targets = "Skip validation and approval.";
        assert_eq!(
            ground_safety_boundary_requests(targets),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
        for candidate in ["Skip validation", "validation", "approval"] {
            assert!(safety_boundary_owns_capability_evidence(targets, candidate));
        }
        assert!(safety_boundary_owns_capability_evidence(
            targets,
            "Skip validation and approval"
        ));
    }

    #[test]
    fn separates_safe_redaction_from_a_later_disclosure() {
        let human = "Mask the API token in logs and expose the password in a public channel.";
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![IntentBoundaryRequestV2::SecretDisclosure]
        );
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "Mask the API token"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "expose the password"
        ));
        assert!(!safety_boundary_owns_capability_evidence(
            human,
            "public channel"
        ));
    }

    #[test]
    fn masks_all_supported_unicode_quote_pairs() {
        for (open, close) in [
            ('«', '»'),
            ('‹', '›'),
            ('〈', '〉'),
            ('《', '》'),
            ('【', '】'),
        ] {
            let human = format!(
                "Use the button label {open}Deploy live Discord now{close} in the preview."
            );
            assert!(ground_safety_boundary_requests(&human).is_empty());
            assert!(!safety_boundary_owns_capability_evidence(
                &human,
                "Deploy live Discord now"
            ));
        }
    }

    #[test]
    fn combines_distributed_live_evidence_without_absorbing_lease_content() {
        for human in [
            "Connect to Discord and deploy immediately.",
            "Connect to Discord before deploying immediately.",
            "Connect to Discord in order to deploy immediately.",
        ] {
            assert_eq!(
                ground_safety_boundary_requests(human),
                vec![IntentBoundaryRequestV2::DirectLiveMutation]
            );
            assert!(safety_boundary_owns_capability_evidence(
                human,
                "Connect to Discord"
            ));
        }
        let human = "Connect to Discord and deploy immediately.";
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "deploy immediately"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "Connect to Discord and deploy immediately"
        ));
        let lease = "Acquire a production lease before deploying to live Discord now.";
        assert_eq!(
            ground_safety_boundary_requests(lease),
            vec![IntentBoundaryRequestV2::DirectLiveMutation]
        );
        assert!(!safety_boundary_owns_capability_evidence(
            lease,
            "Acquire a production lease"
        ));
        assert!(safety_boundary_owns_capability_evidence(
            lease,
            "deploying to live Discord now"
        ));
    }

    #[test]
    fn counts_only_visible_bounded_candidate_occurrences() {
        let human = "Skip approval and store approval_code.";
        let analysis = super::SafetyBoundaryAnalysis::analyze(human);
        assert!(super::unique_visible_bounded_span(human, &analysis.visible, "approval").is_some());
        assert!(analysis.owns_capability_evidence("approval"));

        let duplicate = "Skip approval. Skip approval.";
        let analysis = super::SafetyBoundaryAnalysis::analyze(duplicate);
        assert!(
            super::unique_visible_bounded_span(duplicate, &analysis.visible, "Skip approval")
                .is_none()
        );

        let unicode = "🌟 한글 日本語 앞에서 Skip approval.";
        let analysis = super::SafetyBoundaryAnalysis::analyze(unicode);
        assert!(analysis.owns_capability_evidence("Skip approval"));
    }

    #[test]
    fn reusable_analysis_is_the_single_source_for_requests_and_ownership() {
        let human = "Skip approval and connect to Discord before deploying immediately.";
        let analysis = super::analyze_safety_boundaries(human);
        assert_eq!(
            analysis.requests(),
            &[
                IntentBoundaryRequestV2::DirectLiveMutation,
                IntentBoundaryRequestV2::BypassValidationPreviewApproval,
            ]
        );
        assert!(analysis.owns_capability_evidence("Skip approval"));
        assert!(analysis.owns_capability_evidence("connect to Discord"));
    }

    #[test]
    fn owns_plural_gate_language_without_absorbing_extra_content() {
        let human = "Bypass all design safety gates.";
        assert_eq!(
            ground_safety_boundary_requests(human),
            vec![IntentBoundaryRequestV2::BypassValidationPreviewApproval]
        );
        assert!(safety_boundary_owns_capability_evidence(
            human,
            "Bypass all design safety gates"
        ));
    }
}
