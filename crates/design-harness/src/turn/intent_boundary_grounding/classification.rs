use super::super::intent_interpretation::IntentBoundaryRequestV2;
use super::super::intent_safety_control_grammar::{
    action_permission_length, closed_active_actor_safety_control_meaning,
    closed_actor_safety_control_meaning, closed_configuration_safety_control_meaning,
    closed_direct_separable_turn_off_action, closed_inverted_subject_safety_control_meaning,
    closed_korean_safety_control_clause, closed_passive_target_safety_control_meaning,
    closed_safety_control_action_meaning, closed_safety_control_action_tail,
    closed_safety_control_result_meaning, closed_safety_control_scope,
    closed_safety_control_state_meaning, closed_safety_control_tail,
    closed_separable_turn_off_safety_control_meaning, closed_subject_safety_control_meaning,
    closed_without_safety_control_meaning, preservation_prohibition_length, safety_control_action,
    safety_control_target_length, strip_safety_control_target_modifiers, KoreanSafetyControlClause,
    SafetyControlActionEffect, SafetyControlMeaning, SafetyControlTailEffect,
};
pub(super) use super::super::intent_safety_control_grammar::{
    ACTION_NEGATION_MODIFIERS, ACTION_POLARITY_TOKEN_WINDOW,
    CLOSED_SAFETY_CONTROL_SCOPE_PREPOSITIONS, CLOSED_SAFETY_CONTROL_SCOPE_TERMS,
    CLOSED_SAFETY_CONTROL_TARGETS, CLOSED_SAFETY_CONTROL_TARGET_TERMS, ORDINARY_PREFIX_NEGATIONS,
    PRESERVATION_ACTOR_TERMS, PRESERVATION_DETERMINERS, PRESERVATION_PREFIX_NEGATIONS,
    SAFETY_CONTROL_TARGET_MODIFIERS,
};
use super::syntax::{self, word_continuation, BoundaryUnit, TextSpan, UnitLink};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static SECRET_ACTION_POLARITY_WORK: Cell<usize> = const { Cell::new(0) };
    static MAXIMAL_SECRET_TARGET_WORK: Cell<usize> = const { Cell::new(0) };
    static UNPROTECTED_SECRET_PREFIX_STEPS: Cell<usize> = const { Cell::new(0) };
    static ROOT_SAFETY_CONTROL_PREFIX_STEPS: Cell<usize> = const { Cell::new(0) };
    static ROOT_SAFETY_CONTROL_ACTION_PROBES: Cell<usize> = const { Cell::new(0) };
}

pub(super) const GATE_TARGETS: &[&str] = &[
    "user approval",
    "user-approval",
    "human approval",
    "human confirmation step",
    "review gate",
    "consent",
    "approval checkpoint",
    "validation/preview stage",
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
    "bypasses",
    "bypassing",
    "skip",
    "skips",
    "skipping",
    "omit",
    "omits",
    "omitting",
    "disable",
    "disables",
    "disabling",
    "turns off",
    "turning off",
    "turn off",
    "ignore",
    "ignores",
    "ignoring",
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
pub(super) const GATE_DESTRUCTIVE_ACTIONS: &[&str] = &[
    "drop",
    "drops",
    "dropping",
    "eliminate",
    "eliminates",
    "eliminating",
    "remove",
    "removes",
    "removing",
];
pub(super) const GATE_REQUIREMENT_REVERSAL_ACTIONS: &[&str] = &[
    "enforce",
    "enforces",
    "enforcing",
    "require",
    "requires",
    "requiring",
];
pub(super) const GATE_REQUIREMENT_REVERSAL_PREDICATES: &[&str] =
    &["is disabled", "is forbidden", "is prohibited"];
pub(super) const GATE_RESULT_ACTIONS: &[&str] = &[
    "accept",
    "approve",
    "assume",
    "auto-approve",
    "default",
    "implicit",
    "mark",
    "no-op",
    "noop",
    "passed",
    "set",
    "successful",
    "treat",
];
pub(super) const GATE_ACTION_PERMISSION_PREDICATES: &[&str] = &[
    "is allowed",
    "is enabled",
    "is not disabled",
    "is not forbidden",
    "is not prohibited",
    "is permitted",
];
pub(super) const GATE_EXACT_ACTIONS: &[&str] = &[
    "make approval optional",
    "make user approval optional",
    "make the user approval optional",
    "approval is optional",
    "user approval is optional",
    "approval is not needed",
    "user approval is not needed",
    "approval is not required",
    "user approval is not required",
    "approval isn't required",
    "user approval isn't required",
    "approval isnt required",
    "user approval isnt required",
    "approval isn’t required",
    "user approval isn’t required",
    "do not require user approval",
    "do not require approval",
    "don't require user approval",
    "don't require approval",
    "dont require user approval",
    "dont require approval",
    "don’t require user approval",
    "don’t require approval",
    "require no user approval",
    "require no approval",
    "승인을 빼줘",
    "승인만 빼줘",
    "사용자 승인을 빼줘",
    "승인을 없애줘",
    "승인만 없애줘",
    "사용자 승인을 없애줘",
    "승인을 빼주세요",
    "승인만 빼주세요",
    "사용자 승인을 빼주세요",
    "승인을 없애주세요",
    "승인만 없애주세요",
    "사용자 승인을 없애주세요",
    "승인을 요구하지 마",
    "사용자 승인을 요구하지 마",
    "승인을 요구하지마",
    "사용자 승인을 요구하지마",
    "승인을 요구하지 마세요",
    "사용자 승인을 요구하지 마세요",
    "승인을 요구하지마세요",
    "사용자 승인을 요구하지마세요",
];
pub(super) const GATE_EXACT_ACTION_TERMS: &[&str] = &[
    "allow",
    "allowed",
    "are",
    "be",
    "bypassed",
    "disabled",
    "disallowed",
    "do",
    "does",
    "doesn't",
    "doesnt",
    "doesn’t",
    "don't",
    "dont",
    "don’t",
    "enabled",
    "enforce",
    "enforcing",
    "forbidden",
    "is",
    "isn't",
    "isnt",
    "isn’t",
    "make",
    "may",
    "must",
    "needed",
    "no",
    "not",
    "nobody",
    "one",
    "optional",
    "permitted",
    "permit",
    "proceed",
    "prohibited",
    "require",
    "required",
    "requiring",
    "ignored",
    "omitted",
    "removed",
    "skipped",
    "off",
    "turn",
    "turned",
    "turning",
    "turns",
    "should",
    "will",
];
pub(super) const GATE_EXACT_PREFIX_WRAPPERS: &[&str] = &[
    "please carry out ",
    "carry out ",
    "do this: ",
    "execute ",
    "execute: ",
    "i want you to ",
    "i need you to ",
    "could you ",
    "would you ",
    "right away ",
    "right now ",
    "can you ",
    "will you ",
    "immediately ",
    "please ",
    "only ",
    "just ",
    "now ",
];
pub(super) const GATE_EXACT_SUFFIX_WRAPPERS: &[&str] = &[
    " right away",
    " right now",
    " immediately",
    " please",
    " only",
    " now",
];
pub(super) const GATE_EXACT_WRAPPER_TERMS: &[&str] = &[
    "also",
    "away",
    "can",
    "could",
    "i",
    "immediately",
    "just",
    "need",
    "now",
    "only",
    "please",
    "right",
    "to",
    "want",
    "will",
    "would",
    "you",
];
pub(super) const LIVE_ACTIONS: &[&str] = &[
    "deploy",
    "deploys",
    "deploying",
    "deployed",
    "apply",
    "applies",
    "applying",
    "applied",
    "execute",
    "executes",
    "executing",
    "executed",
    "run",
    "runs",
    "running",
    "connect",
    "connects",
    "connecting",
    "connected",
    "touch",
    "touches",
    "touching",
    "touched",
    "create",
    "creates",
    "creating",
    "created",
    "delete",
    "deletes",
    "deleting",
    "deleted",
    "modify",
    "modifies",
    "modifying",
    "modified",
    "change",
    "changes",
    "changing",
    "changed",
    "publish",
    "publishes",
    "publishing",
    "published",
    "post",
    "posts",
    "posting",
    "posted",
    "grant",
    "grants",
    "granting",
    "granted",
    "revoke",
    "revokes",
    "revoking",
    "revoked",
    "rename",
    "renames",
    "renaming",
    "renamed",
    "edit",
    "edits",
    "editing",
    "edited",
    "update",
    "updates",
    "updating",
    "updated",
    "ban",
    "bans",
    "banning",
    "banned",
    "kick",
    "kicks",
    "kicking",
    "kicked",
    "send",
    "sends",
    "sending",
    "sent",
    "pin",
    "pins",
    "pinning",
    "pinned",
    "move",
    "moves",
    "moving",
    "moved",
    "push",
    "pushes",
    "pushing",
    "pushed",
    "activate",
    "activates",
    "activating",
    "activated",
    "remove",
    "removes",
    "removed",
    "assign",
    "assigns",
    "assigned",
    "give",
    "gives",
    "given",
    "strip",
    "strips",
    "stripped",
    "mute",
    "mutes",
    "muted",
    "timeout",
    "timeouts",
    "timed out",
    "unban",
    "unbans",
    "unbanned",
    "lock",
    "locks",
    "locked",
    "archive",
    "archives",
    "archived",
    "set",
    "sets",
    "add",
    "adds",
    "added",
    "upload",
    "uploads",
    "uploaded",
    "invite",
    "invites",
    "invited",
    "purge",
    "purges",
    "purged",
    "clear",
    "clears",
    "cleared",
    "unpin",
    "unpins",
    "unpinned",
    "unpublish",
    "unpublishes",
    "unpublished",
    "synchronize",
    "synchronizes",
    "synchronized",
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
    "올리",
    "올려",
    "추가",
    "지우",
    "지워",
    "바꾸",
    "바꿔",
    "반영",
    "돌리",
    "돌려",
];
const LIVE_THIRD_PERSON_ACTIONS: &[&str] = &[
    "activates",
    "adds",
    "applies",
    "archives",
    "assigns",
    "bans",
    "changes",
    "clears",
    "connects",
    "creates",
    "deletes",
    "deploys",
    "edits",
    "executes",
    "gives",
    "grants",
    "invites",
    "kicks",
    "locks",
    "modifies",
    "moves",
    "mutes",
    "pins",
    "posts",
    "publishes",
    "purges",
    "pushes",
    "renames",
    "removes",
    "revokes",
    "runs",
    "sends",
    "sets",
    "strips",
    "synchronizes",
    "timeouts",
    "touches",
    "updates",
    "unbans",
    "unpins",
    "unpublishes",
    "uploads",
];
const LIVE_BASE_ACTIONS: &[&str] = &[
    "activate",
    "add",
    "apply",
    "archive",
    "assign",
    "ban",
    "change",
    "clear",
    "connect",
    "create",
    "delete",
    "deploy",
    "edit",
    "execute",
    "give",
    "grant",
    "invite",
    "kick",
    "lock",
    "modify",
    "move",
    "mute",
    "pin",
    "post",
    "publish",
    "purge",
    "push",
    "remove",
    "rename",
    "revoke",
    "run",
    "send",
    "set",
    "strip",
    "synchronize",
    "timeout",
    "touch",
    "unban",
    "unpin",
    "unpublish",
    "update",
    "upload",
];
const LIVE_PASSIVE_ACTIONS: &[&str] = &[
    "activated",
    "added",
    "applied",
    "archived",
    "assigned",
    "banned",
    "changed",
    "cleared",
    "connected",
    "created",
    "deleted",
    "deployed",
    "edited",
    "executed",
    "given",
    "granted",
    "invited",
    "kicked",
    "locked",
    "modified",
    "moved",
    "muted",
    "pinned",
    "posted",
    "published",
    "purged",
    "pushed",
    "renamed",
    "removed",
    "revoked",
    "run",
    "sent",
    "set",
    "stripped",
    "synchronized",
    "timed out",
    "touched",
    "updated",
    "unbanned",
    "unpinned",
    "unpublished",
    "uploaded",
];
pub(super) const LIVE_CONTEXT: &[&str] = &[
    "live-discord",
    "live environment",
    "live guild",
    "live discord",
    "live server",
    "production discord",
    "production environment",
    "production guild",
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
pub(super) const LIVE_CONTEXT_ALIASES: &[&str] = &["prod", "production"];
const LIVE_MUTABLE_RESOURCE_TARGETS: &[&str] = &[
    "category",
    "categories",
    "channel",
    "channels",
    "command",
    "commands",
    "emoji",
    "emojis",
    "guild",
    "guilds",
    "invite",
    "invites",
    "member",
    "members",
    "message",
    "messages",
    "panel",
    "panels",
    "permission",
    "permissions",
    "role",
    "roles",
    "server",
    "servers",
    "sticker",
    "stickers",
    "thread",
    "threads",
    "webhook",
    "webhooks",
];
const LIVE_RESOURCE_DESCRIPTION_TARGETS: &[&str] = &[
    "analysis",
    "diagram",
    "documentation",
    "guide",
    "guidance",
    "metrics",
    "preview",
    "report",
    "snapshot",
    "status",
    "tutorial",
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
    "secrets",
    "api key",
    "api keys",
    "api-key",
    "api-keys",
    "api_key",
    "api_keys",
    "api token",
    "api tokens",
    "api-token",
    "api-tokens",
    "api_token",
    "api_tokens",
    "bearer token",
    "bearer tokens",
    "connection string",
    "connection strings",
    "cookie",
    "cookies",
    "database connection string",
    "database connection strings",
    "dsn",
    "dsns",
    "encryption key",
    "encryption keys",
    "jwt",
    "jwts",
    "private key",
    "private keys",
    "session cookie",
    "session cookies",
    "session token",
    "session tokens",
    "signing key",
    "signing keys",
    "ssh key",
    "ssh keys",
    "access key",
    "access keys",
    "access token",
    "access tokens",
    "access-token",
    "access-tokens",
    "token",
    "tokens",
    "credential",
    "credentials",
    "password",
    "passwords",
    "environment variable",
    "environment variables",
    "env var",
    "env vars",
    "비밀",
    "api 키",
    "api키",
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
    "exposes",
    "exposing",
    "exposed",
    "reveal",
    "reveals",
    "revealing",
    "revealed",
    "disclose",
    "discloses",
    "disclosing",
    "disclosed",
    "publish",
    "publishes",
    "publishing",
    "published",
    "post",
    "posts",
    "posting",
    "posted",
    "show",
    "shows",
    "showing",
    "shown",
    "display",
    "displays",
    "displaying",
    "displayed",
    "print",
    "prints",
    "printing",
    "printed",
    "include",
    "includes",
    "including",
    "included",
    "contains",
    "contained",
    "appears",
    "present",
    "put",
    "puts",
    "send",
    "sends",
    "sending",
    "sent",
    "leak",
    "leaks",
    "leaking",
    "leaked",
    "go",
    "goes",
    "going",
    "went",
    "gone",
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
    "보내",
    "공유",
    "올리",
    "올라가",
    "적",
    "알리",
];
const SECRET_THIRD_PERSON_ACTIONS: &[&str] = &[
    "appears",
    "contains",
    "discloses",
    "displays",
    "exposes",
    "goes",
    "includes",
    "leaks",
    "posts",
    "prints",
    "publishes",
    "puts",
    "reveals",
    "sends",
    "shows",
];
const SECRET_BASE_ACTIONS: &[&str] = &[
    "disclose", "display", "expose", "go", "include", "leak", "post", "print", "publish", "put",
    "reveal", "send", "show",
];
const SECRET_PASSIVE_ACTIONS: &[&str] = &[
    "contained",
    "disclosed",
    "displayed",
    "exposed",
    "gone",
    "included",
    "leaked",
    "posted",
    "printed",
    "published",
    "put",
    "present",
    "revealed",
    "sent",
    "shown",
];
pub(super) const CLOSED_THIRD_PERSON_BOUNDARY_ACTORS: &[&str] = &[
    "agent",
    "agents",
    "alert",
    "alerts",
    "application",
    "applications",
    "automation",
    "automations",
    "bot",
    "bots",
    "detector",
    "detectors",
    "handler",
    "handlers",
    "job",
    "jobs",
    "process",
    "processes",
    "rule",
    "rules",
    "service",
    "services",
    "server",
    "servers",
    "system",
    "systems",
    "worker",
    "workers",
    "workflow",
    "workflows",
    "warning",
    "warnings",
];
pub(super) const CLOSED_SECRET_DISCLOSURE_ACTORS: &[&str] = &[
    "embed",
    "embeds",
    "message",
    "messages",
    "panel",
    "panels",
    "post",
    "posts",
    "response",
    "responses",
];
pub(super) const CLOSED_BOUNDARY_ACTION_ADVERBS: &[&str] = &[
    "also",
    "always",
    "automatically",
    "currently",
    "deliberately",
    "directly",
    "first",
    "firstly",
    "immediately",
    "just",
    "next",
    "now",
    "only",
    "publicly",
    "remotely",
    "then",
    "afterward",
    "afterwards",
    "up",
];
pub(super) const SAFE_REDACTION: &[&str] = &[
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
    "redaction disabled",
    "masking disabled",
    "mask=false",
    "mask = false",
    "가리지 말",
    "가림 없이",
    "마스킹 없이",
    "대체하지 말",
    "숨기지 말",
];
pub(super) const SECRET_DELIVERY_CONTEXT: &[&str] = &[
    "public",
    "publicly",
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
pub(super) const GATE_CONTROL_TERMS: &[&str] = &[
    "all",
    "any",
    "avoid",
    "disallow",
    "disallowing",
    "do",
    "don't",
    "dont",
    "don’t",
    "forbid",
    "forbidding",
    "from",
    "hesitate",
    "must",
    "mustn't",
    "mustn’t",
    "never",
    "no",
    "not",
    "prevent",
    "preventing",
    "refuse",
    "should",
    "shouldn't",
    "shouldn’t",
    "stop",
    "the",
    "without",
    "안",
    "절대",
    "금지",
    "못",
];
pub(super) const SUFFIX_NEGATIONS: &[&str] = &[
    "not",
    "n't",
    "is forbidden",
    "is prohibited",
    "is disabled",
    "하지",
    "하지마",
    "하지마세요",
    "하지말고",
    "하지 마",
    "하지 말고",
    "하지 마세요",
    "하지 않",
    "하지 않고",
    "지 않",
    "지 않고",
    "지마",
    "지마세요",
    "지 마",
    "지 말고",
    "지 마세요",
    "지 말",
    "않아",
    "않고",
    "말아",
    "말고",
    "금지",
    "안 해",
    "안 함",
    "못 해",
    "를 막",
    "을 막",
    "를 금지",
    "을 금지",
    "되지 않게",
    "되지 못하게",
    "지 않게",
    "지 못하게",
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
    pub(super) secret_unsafe_target: bool,
    pub(super) secret_delivery: bool,
    pub(super) secret_unprotected: bool,
}

impl UnitFacts {
    pub(super) fn for_text(value: &str) -> Self {
        let gate_meaning = closed_gate_control_meaning(value);
        let gate_result_meaning =
            closed_safety_control_result_meaning(&value.split_whitespace().collect::<Vec<_>>());
        let korean_gate = korean_safety_control_clause(value);
        let gate_preservation = gate_meaning == Some(SafetyControlMeaning::PreservesControl)
            || matches!(
                korean_gate,
                Some(KoreanSafetyControlClause::Control(
                    SafetyControlMeaning::PreservesControl
                )) | Some(KoreanSafetyControlClause::BusinessOperation)
            );
        let secret_target = contains_bounded_any(value, SECRET_TARGETS);
        let secret_unprotected = has_unnegated_unprotected_secret(value);
        Self {
            gate_action: !gate_preservation
                && (gate_meaning == Some(SafetyControlMeaning::WeakensControl)
                    || korean_gate
                        == Some(KoreanSafetyControlClause::Control(
                            SafetyControlMeaning::WeakensControl,
                        ))
                    || has_optional_gate_bypass_text(value)
                    || closed_requested_rule_gate_weakening(value)),
            gate_target: has_bounded_gate_target(value) || gate_result_meaning.is_some(),
            live_action: has_unnegated_boundary_action(value, BoundaryKind::Live),
            live_strong_context: has_operational_live_context(value)
                || contains_bounded_any(value, &["live changes", "라이브 변경"]),
            live_weak_context: contains_bounded_any(value, live_weak_context()),
            immediate: contains_bounded_any(value, IMMEDIATE_CONTEXT),
            secret_action: has_unnegated_boundary_action(value, BoundaryKind::Secret),
            secret_target,
            secret_unsafe_target: has_unsafe_secret_target(value)
                || (secret_target && secret_unprotected),
            secret_delivery: contains_bounded_any(value, SECRET_DELIVERY_CONTEXT),
            secret_unprotected,
        }
    }

    pub(super) fn for_unit(unit: &BoundaryUnit) -> Self {
        let mut facts = Self::for_text(&unit.text);
        facts.gate_action &= !unit.inherited_gate_action_negation;
        facts.live_action &= !unit.inherited_live_action_negation;
        facts.secret_action &= !unit.inherited_secret_action_negation;
        facts
    }

    pub(super) fn is_seed(&self, kind: BoundaryKind) -> bool {
        match kind {
            BoundaryKind::Gate => self.gate_action && self.gate_target,
            BoundaryKind::Live => {
                self.live_action
                    && (self.live_strong_context || (self.live_weak_context && self.immediate))
            }
            BoundaryKind::Secret => {
                (self.secret_action && self.secret_unsafe_target)
                    || (self.secret_unsafe_target
                        && self.secret_unprotected
                        && self.secret_delivery)
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
                    || self.secret_unsafe_target
                    || self.secret_delivery
                    || self.secret_unprotected
            }
        }
    }
}

fn closed_requested_rule_gate_weakening(value: &str) -> bool {
    if !closed_requested_observer_artifact(value) {
        return false;
    }
    GATE_ACTIONS.iter().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            let end = start.saturating_add(matched.len());
            marker_has_boundaries(value, start, end)
                && !marker_is_negated(value, start, end)
                && value[..start].split_whitespace().next_back() == Some("that")
                && has_bounded_gate_target(&value[end..])
        })
    })
}

pub(super) fn classify_sentence_units(
    visible: &[char],
    sentence: TextSpan,
    question: bool,
) -> Vec<BoundaryUnit> {
    let (mut units, operative_start) = syntax::sentence_units(visible, sentence, question);
    merge_third_person_actor_tails(visible, &mut units);
    merge_gate_business_tails(visible, &mut units);
    merge_gate_exception_tails(visible, &mut units);
    apply_hypothetical_scope(&mut units, question);
    apply_non_authoritative_event_scope(visible, &mut units);
    apply_coordinated_negation_scope(&mut units);
    if let Some(operative_start) = operative_start {
        for unit in &mut units {
            unit.hypothetical = unit.span.start < operative_start;
        }
    }
    units
}

fn apply_non_authoritative_event_scope(visible: &[char], units: &mut [BoundaryUnit]) {
    for index in 0..units.len() {
        if closed_non_authoritative_event_unit(&units[index].text) {
            units[index].hypothetical = true;
            units[index].non_authoritative_event = true;
        }
        let Some(previous) = index.checked_sub(1).and_then(|index| units.get(index)) else {
            continue;
        };
        let connector = visible[previous.span.end..units[index].span.start]
            .iter()
            .collect::<String>()
            .to_lowercase();
        if units[index].link == UnitLink::Barrier
            && matches!(connector.trim(), "when" | "whenever" | "while")
            && closed_requested_observer_artifact(&previous.text)
        {
            units[index].hypothetical = true;
            units[index].non_authoritative_event = true;
        }
    }
}

fn closed_non_authoritative_event_unit(value: &str) -> bool {
    let observer_event = [" attempts to ", " attempt to ", " tries to "]
        .iter()
        .find_map(|connector| value.find(connector))
        .is_some_and(|event_start| {
            closed_observer_guard_carrier(&value[..event_start])
                && !unit_text_is_boundary_seed(&value[..event_start])
        });
    let observed_trigger = value.find(" whenever ").is_some_and(|event_start| {
        closed_requested_observer_artifact(&value[..event_start])
            && !unit_text_is_boundary_seed(&value[..event_start])
    });
    let simulated_event = [
        "simulate ",
        "simulates ",
        "simulated ",
        "simulating ",
        "simulation of ",
    ]
    .iter()
    .any(|carrier| value.starts_with(carrier));
    observer_event || observed_trigger || simulated_event
}

fn closed_observer_guard_carrier(value: &str) -> bool {
    let artifact = [
        "alert",
        "alerts",
        "detector",
        "detectors",
        "guard",
        "guards",
        "monitor",
        "monitors",
        "rule",
        "rules",
        "warning",
        "warnings",
    ]
    .iter()
    .any(|marker| contains_bounded_any(value, &[*marker]));
    let predicate = [
        "alert", "alerts", "block", "blocks", "denies", "deny", "detect", "detects", "monitor",
        "monitors", "prevent", "prevents", "warn", "warns",
    ]
    .iter()
    .any(|marker| contains_bounded_any(value, &[*marker]));
    artifact && predicate
}

fn closed_requested_observer_artifact(value: &str) -> bool {
    let artifact = [
        "alert",
        "alerts",
        "detector",
        "detectors",
        "guard",
        "guards",
        "monitor",
        "monitors",
        "rule",
        "rules",
        "warning",
        "warnings",
    ]
    .iter()
    .any(|marker| contains_bounded_any(value, &[*marker]));
    let requested = ["build", "create", "make"]
        .iter()
        .any(|marker| contains_bounded_any(value, &[*marker]));
    artifact && requested
}

fn unit_text_is_boundary_seed(value: &str) -> bool {
    let facts = UnitFacts::for_text(value);
    [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret]
        .into_iter()
        .any(|kind| facts.is_seed(kind))
        || (has_bounded_gate_target(value)
            && contains_bounded_any(
                value,
                &[
                    GATE_ACTIONS,
                    GATE_DESTRUCTIVE_ACTIONS,
                    GATE_REQUIREMENT_REVERSAL_ACTIONS,
                ]
                .concat(),
            ))
}

fn merge_third_person_actor_tails(visible: &[char], units: &mut Vec<BoundaryUnit>) {
    let original = std::mem::take(units);
    let mut merged = Vec::with_capacity(original.len());
    let mut index = 0usize;
    while index < original.len() {
        let tail_kind = original
            .get(index.saturating_add(1))
            .filter(|unit| unit.link == UnitLink::Sequential)
            .and_then(|unit| starts_with_third_person_boundary_action(&unit.text));
        if tail_kind.is_some_and(|kind| closed_boundary_actor_unit(&original[index].text, kind)) {
            let mut unit = original[index].clone();
            unit.span.end = original[index + 1].span.end;
            unit.text = syntax::normalized_text(
                &visible[unit.span.start..unit.span.end]
                    .iter()
                    .collect::<String>()
                    .to_lowercase(),
            );
            merged.push(unit);
            index = index.saturating_add(2);
        } else {
            merged.push(original[index].clone());
            index = index.saturating_add(1);
        }
    }
    *units = merged;
}

fn closed_boundary_actor_unit(value: &str, kind: BoundaryKind) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    let actor = match words.as_slice() {
        [actor] => Some(*actor),
        ["a" | "an" | "the", actor] => Some(*actor),
        ["a" | "an" | "the", "public", actor] => Some(*actor),
        _ => None,
    };
    actor.is_some_and(|actor| closed_third_person_actor(kind, actor))
}

fn closed_public_secret_disclosure_subject(value: &str, action_start: usize) -> bool {
    if !contains_bounded_any(value, SECRET_TARGETS)
        || !contains_bounded_any(value, SECRET_DELIVERY_CONTEXT)
    {
        return false;
    }
    let prefix = value[..action_start].trim_end();
    let mut words = prefix.split_whitespace().collect::<Vec<_>>();
    while words
        .last()
        .is_some_and(|word| closed_boundary_action_adverb(word))
    {
        words.pop();
    }
    if !words
        .first()
        .is_some_and(|word| matches!(*word, "a" | "an" | "the"))
    {
        return false;
    }
    words.remove(0);
    !words.is_empty()
        && words.len() <= 4
        && words.iter().all(|word| {
            word.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !matches!(
                    *word,
                    "can"
                        | "could"
                        | "do"
                        | "does"
                        | "may"
                        | "might"
                        | "must"
                        | "not"
                        | "should"
                        | "will"
                        | "would"
                )
        })
}

fn starts_with_third_person_boundary_action(value: &str) -> Option<BoundaryKind> {
    let mut words = value.split_whitespace();
    let mut word = words.next()?;
    for _ in 0..2 {
        if !closed_boundary_action_adverb(word) {
            break;
        }
        word = words.next()?;
    }
    if LIVE_THIRD_PERSON_ACTIONS.contains(&word) {
        Some(BoundaryKind::Live)
    } else if SECRET_THIRD_PERSON_ACTIONS.contains(&word) {
        Some(BoundaryKind::Secret)
    } else {
        None
    }
}

fn merge_gate_exception_tails(visible: &[char], units: &mut Vec<BoundaryUnit>) {
    let original = std::mem::take(units);
    let mut merged = Vec::with_capacity(original.len());
    let mut index = 0usize;
    while index < original.len() {
        if index.saturating_add(1) < original.len()
            && original[index + 1].link == UnitLink::ConditionalAlternative
            && closed_gate_control_meaning(&original[index].text)
                == Some(SafetyControlMeaning::PreservesControl)
            && closed_gate_exception_scope(&original[index + 1].text)
        {
            let mut unit = original[index].clone();
            unit.span.end = original[index + 1].span.end;
            unit.text = syntax::normalized_text(
                &visible[unit.span.start..unit.span.end]
                    .iter()
                    .collect::<String>()
                    .to_lowercase(),
            );
            merged.push(unit);
            index = index.saturating_add(2);
        } else {
            merged.push(original[index].clone());
            index = index.saturating_add(1);
        }
    }
    *units = merged;
}

fn merge_gate_business_tails(visible: &[char], units: &mut Vec<BoundaryUnit>) {
    let original = std::mem::take(units);
    let mut merged: Vec<BoundaryUnit> = Vec::with_capacity(original.len());
    let mut index = 0usize;
    while index < original.len() {
        let Some(end) = gate_business_run(&original, index) else {
            merged.push(original[index].clone());
            index = index.saturating_add(1);
            continue;
        };
        let mut unit = original[index].clone();
        unit.span.end = original[end].span.end;
        unit.text = syntax::normalized_text(
            &visible[unit.span.start..unit.span.end]
                .iter()
                .collect::<String>()
                .to_lowercase(),
        );
        merged.push(unit);
        index = end.saturating_add(1);
    }
    *units = merged;
}

fn gate_business_run(units: &[BoundaryUnit], start: usize) -> Option<usize> {
    let first = strip_exact_prefix_wrappers(&units.get(start)?.text);
    let first_words = first.split_whitespace().collect::<Vec<_>>();
    let action = safety_control_action(&first_words)?;
    if !closed_safety_control_tail(&first_words[action.length..]) {
        return None;
    }
    for (index, unit) in units.iter().enumerate().skip(start.saturating_add(1)) {
        if unit.link != UnitLink::Additive || closed_gate_control_meaning(&unit.text).is_some() {
            return None;
        }
        let words = unit.text.split_whitespace().collect::<Vec<_>>();
        let target = strip_safety_control_target_modifiers(&words);
        let target_length = safety_control_target_length(target)?;
        let remainder = &target[target_length..];
        if remainder.is_empty() {
            continue;
        }
        return closed_gate_business_object_tail(first_words[0], remainder).then_some(index);
    }
    None
}

fn closed_gate_business_object_tail(action: &str, words: &[&str]) -> bool {
    match action {
        "drop" | "dropping" | "eliminate" | "eliminating" | "remove" | "removing" => {
            matches!(
                words,
                [
                    "artifact"
                        | "artifacts"
                        | "event"
                        | "events"
                        | "log"
                        | "logs"
                        | "record"
                        | "records"
                        | "request"
                        | "requests"
                        | "status",
                    ..
                ] | [
                    "audit",
                    "log" | "logging" | "logs" | "record" | "records",
                    ..
                ]
            )
        }
        "ignore" | "ignoring" => matches!(
            words,
            ["error" | "handling" | "latency" | "log" | "logs", ..]
        ),
        "disable" | "disabling" => matches!(
            words,
            [
                "animation" | "animations" | "notification" | "notifications",
                ..
            ]
        ),
        "omit" | "omitting" | "skip" | "skipping" => matches!(
            words,
            [
                "audit",
                "log" | "logging" | "logs" | "record" | "records",
                ..
            ]
        ),
        _ => false,
    }
}

fn apply_hypothetical_scope(units: &mut [BoundaryUnit], question: bool) {
    let mut inherited = question;
    let mut local = false;
    for unit in units {
        if matches!(unit.link, UnitLink::Sequential | UnitLink::Barrier) {
            local = false;
        }
        let explicit_hypothetical = contains_hypothetical_marker(&unit.text);
        let explicit_local = contains_local_discussion_marker(&unit.text);
        let explicit_assertion = contains_polite_request(&unit.text);
        if explicit_hypothetical {
            inherited = true;
        } else if explicit_local {
            local = true;
        } else if explicit_assertion {
            inherited = false;
            local = false;
        }
        unit.hypothetical = inherited || local;
    }
}

fn contains_local_discussion_marker(value: &str) -> bool {
    let value = strip_exact_prefix_wrappers(value);
    [
        "describe the risk ",
        "describe the risks ",
        "discuss ",
        "discussion only:",
        "explain the tradeoff ",
        "explain the tradeoffs ",
        "explain whether ",
        "let's discuss ",
        "let us discuss ",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn apply_coordinated_negation_scope(units: &mut [BoundaryUnit]) {
    if units.len() < 2 {
        return;
    }
    let mut inherited = [false; 3];
    let mut coordinated_preservation = [None; 3];
    let mut alternative_negation = AlternativeNegationScope::None;
    for unit in units {
        let connected = matches!(
            unit.link,
            UnitLink::Additive | UnitLink::Alternative | UnitLink::NegativeAlternative
        );
        let active_alternative_negation = if unit.link == UnitLink::NegativeAlternative {
            AlternativeNegationScope::WholeClause
        } else {
            alternative_negation
        };
        let mut inherited_alternative = false;
        for kind in [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret] {
            let index = boundary_kind_index(kind);
            let inherits_alternative = matches!(
                unit.link,
                UnitLink::Alternative | UnitLink::NegativeAlternative
            ) && match active_alternative_negation {
                AlternativeNegationScope::None => false,
                AlternativeNegationScope::BareOnly => {
                    starts_with_bare_boundary_action(&unit.text, kind)
                }
                AlternativeNegationScope::WholeClause => {
                    starts_with_bare_boundary_action(&unit.text, kind)
                        || independent_positive_boundary_clause(&unit.text, kind)
                        || UnitFacts::for_text(&unit.text).is_seed(kind)
                }
            };
            inherited_alternative |= inherits_alternative;
            if !connected {
                inherited[index] = false;
                coordinated_preservation[index] = None;
            }
            if matches!(
                unit.link,
                UnitLink::Alternative | UnitLink::NegativeAlternative
            ) && !inherits_alternative
            {
                inherited[index] = false;
                coordinated_preservation[index] = None;
            }
            if independent_positive_boundary_clause(&unit.text, kind) {
                inherited[index] = false;
                coordinated_preservation[index] = None;
            }
            if inherits_alternative {
                inherited[index] = true;
                coordinated_preservation[index] = None;
            }
            if let Some(continuation) = direct_preservation_continuation(&unit.text, kind) {
                set_inherited_action_negation(unit, kind, false);
                inherited[index] = false;
                coordinated_preservation[index] = Some(continuation);
                continue;
            }
            let inherits_preservation = connected
                && coordinated_preservation[index].is_some_and(|continuation| {
                    starts_with_preservation_action(&unit.text, kind, continuation)
                });
            if inherits_preservation {
                set_inherited_action_negation(unit, kind, true);
                inherited[index] = false;
                continue;
            }
            set_inherited_action_negation(unit, kind, inherited[index]);
            coordinated_preservation[index] = None;
            if has_negated_boundary_action_with_anchor(&unit.text, kind) {
                inherited[index] = true;
            } else if !inherited_action_negation(unit, kind) {
                inherited[index] = false;
            }
        }
        let opened_scope = leading_alternative_negation_scope(&unit.text);
        alternative_negation = if opened_scope != AlternativeNegationScope::None {
            opened_scope
        } else if matches!(
            unit.link,
            UnitLink::Alternative | UnitLink::NegativeAlternative
        ) && (active_alternative_negation == AlternativeNegationScope::WholeClause
            || inherited_alternative)
        {
            active_alternative_negation
        } else if has_negated_action_marker(&unit.text) && !unit.text.starts_with("either ") {
            AlternativeNegationScope::BareOnly
        } else {
            AlternativeNegationScope::None
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AlternativeNegationScope {
    None,
    BareOnly,
    WholeClause,
}

fn has_negated_action_marker(value: &str) -> bool {
    [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret]
        .into_iter()
        .any(|kind| has_negated_boundary_action_marker(value, kind))
}

fn starts_with_bare_boundary_action(value: &str, kind: BoundaryKind) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    let mut start = 0usize;
    while words
        .get(start)
        .is_some_and(|word| ACTION_NEGATION_MODIFIERS.contains(word))
    {
        start = start.saturating_add(1);
    }
    let words = &words[start..];
    if kind == BoundaryKind::Gate {
        if let Some(action) = safety_control_action(words) {
            return closed_safety_control_action_tail(&words[action.length..])
                == Some(SafetyControlTailEffect::Direct);
        }
        return closed_direct_separable_turn_off_action(words);
    }
    if boundary_governance_predicate(value) {
        return false;
    }
    boundary_action_markers(kind)
        .filter_map(|marker| marker.split_whitespace().next())
        .any(|marker| words.first() == Some(&marker))
}

fn independent_positive_boundary_clause(value: &str, kind: BoundaryKind) -> bool {
    if explicit_positive_request_wrapper(value) {
        return UnitFacts::for_text(value).has_evidence(kind);
    }
    match kind {
        BoundaryKind::Gate => {
            closed_gate_control_meaning(value) == Some(SafetyControlMeaning::WeakensControl)
                && !starts_with_bare_boundary_action(value, kind)
        }
        BoundaryKind::Live | BoundaryKind::Secret => {
            has_unnegated_boundary_action(value, kind)
                && (boundary_governance_predicate(value)
                    || has_explicit_third_person_boundary_action(value, kind))
        }
    }
}

fn has_explicit_third_person_boundary_action(value: &str, kind: BoundaryKind) -> bool {
    let third_person = match kind {
        BoundaryKind::Live => LIVE_THIRD_PERSON_ACTIONS,
        BoundaryKind::Secret => SECRET_THIRD_PERSON_ACTIONS,
        BoundaryKind::Gate => return false,
    };
    third_person.iter().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            let end = start.saturating_add(matched.len());
            marker_has_boundaries(value, start, end)
                && !marker_is_negated(value, start, end)
                && closed_third_person_boundary_actor(value, start, kind)
        })
    })
}

fn explicit_positive_request_wrapper(value: &str) -> bool {
    GATE_EXACT_PREFIX_WRAPPERS
        .iter()
        .filter(|wrapper| {
            matches!(
                **wrapper,
                "i want you to "
                    | "i need you to "
                    | "could you "
                    | "would you "
                    | "can you "
                    | "will you "
                    | "please "
            )
        })
        .any(|wrapper| value.starts_with(wrapper))
}

fn boundary_governance_predicate(value: &str) -> bool {
    [
        " is allowed",
        " is enabled",
        " is permitted",
        " is not disabled",
        " is not forbidden",
        " is not prohibited",
        " can be allowed",
        " may be allowed",
        " must be allowed",
        " should be allowed",
        " will be allowed",
    ]
    .iter()
    .any(|predicate| value.contains(predicate))
}

fn leading_alternative_negation_scope(value: &str) -> AlternativeNegationScope {
    if value.starts_with("either ") {
        return AlternativeNegationScope::None;
    }
    if value.starts_with("neither ") {
        return AlternativeNegationScope::WholeClause;
    }
    let Some(control) = [
        "do not ",
        "don't ",
        "dont ",
        "don’t ",
        "never ",
        "must not ",
        "should not ",
    ]
    .iter()
    .find(|control| value.starts_with(**control)) else {
        return AlternativeNegationScope::None;
    };
    if value[control.len()..].starts_with("either ") {
        AlternativeNegationScope::WholeClause
    } else {
        AlternativeNegationScope::BareOnly
    }
}

fn has_negated_boundary_action_with_anchor(value: &str, kind: BoundaryKind) -> bool {
    match kind {
        BoundaryKind::Gate => {
            closed_gate_control_meaning(value) == Some(SafetyControlMeaning::PreservesControl)
                && marker_sets_have_negated_action(
                    value,
                    &[
                        GATE_ACTIONS,
                        GATE_DESTRUCTIVE_ACTIONS,
                        GATE_EXACT_ACTIONS,
                        GATE_REQUIREMENT_REVERSAL_ACTIONS,
                    ],
                )
        }
        BoundaryKind::Live => marker_sets_have_negated_action(value, &[LIVE_ACTIONS]),
        BoundaryKind::Secret => marker_sets_have_negated_action(value, &[SECRET_ACTIONS]),
    }
}

fn boundary_kind_index(kind: BoundaryKind) -> usize {
    match kind {
        BoundaryKind::Gate => 0,
        BoundaryKind::Live => 1,
        BoundaryKind::Secret => 2,
    }
}

pub(super) fn inherited_action_negation(unit: &BoundaryUnit, kind: BoundaryKind) -> bool {
    match kind {
        BoundaryKind::Gate => unit.inherited_gate_action_negation,
        BoundaryKind::Live => unit.inherited_live_action_negation,
        BoundaryKind::Secret => unit.inherited_secret_action_negation,
    }
}

fn set_inherited_action_negation(unit: &mut BoundaryUnit, kind: BoundaryKind, value: bool) {
    match kind {
        BoundaryKind::Gate => unit.inherited_gate_action_negation = value,
        BoundaryKind::Live => unit.inherited_live_action_negation = value,
        BoundaryKind::Secret => unit.inherited_secret_action_negation = value,
    }
}

pub(super) fn live_weak_context() -> &'static [&'static str] {
    &["live", "discord", "server", "라이브", "디스코드", "서버"]
}

pub(super) fn contains_any(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
}

fn contains_bounded_any(value: &str, markers: &[&str]) -> bool {
    bounded_marker_occurrences(value, markers).next().is_some()
}

fn has_operational_live_context(value: &str) -> bool {
    bounded_marker_occurrences(value, LIVE_CONTEXT).any(|(start, _)| {
        let (preposition, preceding) = live_context_predecessors(value, start);
        if descriptive_live_context_preposition(preposition) {
            return false;
        }
        if preposition == Some("on")
            && preceding
                .take(6)
                .any(|word| LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&word))
        {
            return false;
        }
        true
    }) || bounded_marker_occurrences(value, LIVE_CONTEXT_ALIASES).any(|(start, end)| {
        let (preposition, preceding) = live_context_predecessors(value, start);
        if descriptive_live_context_preposition(preposition) {
            return false;
        }
        if preposition == Some("on")
            && preceding
                .take(6)
                .any(|word| LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&word))
        {
            return false;
        }
        live_alias_has_mutable_resource(value, end)
            || preposition.is_some_and(|word| {
                matches!(
                    word,
                    "against" | "at" | "from" | "in" | "into" | "on" | "to"
                )
            })
    })
}

fn live_context_predecessors(
    value: &str,
    start: usize,
) -> (Option<&str>, impl Iterator<Item = &str>) {
    let mut preceding = value[..start].split_whitespace().rev();
    let mut preposition = preceding.next();
    while preposition.is_some_and(|word| matches!(word, "a" | "an" | "the")) {
        preposition = preceding.next();
    }
    (preposition, preceding)
}

fn descriptive_live_context_preposition(preposition: Option<&str>) -> bool {
    preposition.is_some_and(|word| {
        matches!(
            word,
            "about" | "concerning" | "describing" | "for" | "of" | "regarding" | "representing"
        )
    })
}

fn live_alias_has_mutable_resource(value: &str, alias_end: usize) -> bool {
    let mut following = value[alias_end..].split_whitespace();
    let Some(resource) = following.next() else {
        return false;
    };
    LIVE_MUTABLE_RESOURCE_TARGETS.contains(&resource)
        && !following
            .next()
            .is_some_and(|word| LIVE_RESOURCE_DESCRIPTION_TARGETS.contains(&word))
}

fn bounded_marker_occurrences<'a>(
    value: &'a str,
    markers: &'a [&'a str],
) -> impl Iterator<Item = (usize, usize)> + 'a {
    markers.iter().flat_map(move |marker| {
        value.match_indices(marker).filter_map(|(start, matched)| {
            let end = start.saturating_add(matched.len());
            marker_has_boundaries(value, start, end).then_some((start, end))
        })
    })
}

fn has_unnegated_unprotected_secret(value: &str) -> bool {
    let action_polarities = secret_action_polarities(value);
    let control_state = UnprotectedSecretControlState::analyze(value);
    let unprotected = source_ordered_bounded_marker_occurrences(value, UNPROTECTED_SECRET);
    let mut action_index = 0usize;
    let mut latest_action = None;
    unprotected.into_iter().any(|(start, end)| {
        let marker = &value[start..end];
        if marker.starts_with("do not ")
            || marker.starts_with("don't ")
            || marker.starts_with("dont ")
            || marker.starts_with("never ")
        {
            return !marker_is_negated(value, start, end);
        }
        while action_polarities
            .get(action_index)
            .is_some_and(|(action_start, _)| *action_start < start)
        {
            latest_action = action_polarities
                .get(action_index)
                .map(|(_, polarity)| *polarity);
            action_index = action_index.saturating_add(1);
        }
        if let Some(unnegated) = latest_action {
            return unnegated;
        }
        !marker_is_negated(value, start, end) && !control_state.clause_negates_before(start)
    })
}

fn source_ordered_bounded_marker_occurrences(value: &str, markers: &[&str]) -> Vec<(usize, usize)> {
    let mut furthest_end_at_start = vec![None; value.len().saturating_add(1)];
    for (start, end) in bounded_marker_occurrences(value, markers) {
        let slot = &mut furthest_end_at_start[start];
        *slot = Some(slot.map_or(end, |current: usize| current.max(end)));
    }
    furthest_end_at_start
        .into_iter()
        .enumerate()
        .filter_map(|(start, end)| end.map(|end| (start, end)))
        .collect()
}

struct UnprotectedSecretControlState<'a> {
    value: &'a str,
    leading_negative_ready_at: Option<usize>,
    preservation_controls: Vec<(&'static str, Option<usize>)>,
}

impl<'a> UnprotectedSecretControlState<'a> {
    fn analyze(value: &'a str) -> Self {
        let trimmed = value.trim_start();
        let trimmed_start = value.len().saturating_sub(trimmed.len());
        let leading_negative_ready_at = ORDINARY_PREFIX_NEGATIONS
            .iter()
            .filter_map(|control| {
                let remainder = trimmed.strip_prefix(control)?;
                remainder
                    .chars()
                    .next()
                    .is_some_and(char::is_whitespace)
                    .then_some(())?;
                remainder
                    .char_indices()
                    .find(|(_, character)| !character.is_whitespace())
                    .map(|(start, character)| {
                        trimmed_start
                            .saturating_add(control.len())
                            .saturating_add(start)
                            .saturating_add(character.len_utf8())
                    })
            })
            .min();
        let preservation_controls = PRESERVATION_PREFIX_NEGATIONS
            .iter()
            .map(|control| {
                (
                    *control,
                    bounded_marker_occurrences(value, &[*control])
                        .next()
                        .map(|(_, end)| end),
                )
            })
            .collect();
        Self {
            value,
            leading_negative_ready_at,
            preservation_controls,
        }
    }

    fn clause_negates_before(&self, before: usize) -> bool {
        #[cfg(test)]
        UNPROTECTED_SECRET_PREFIX_STEPS.with(|steps| {
            steps.set(
                steps
                    .get()
                    .saturating_add(1usize.saturating_add(self.preservation_controls.len())),
            );
        });
        if !self
            .leading_negative_ready_at
            .is_some_and(|ready_at| ready_at <= before)
        {
            return false;
        }
        let prefix = &self.value[..before];
        let preservation_controls = self
            .preservation_controls
            .iter()
            .filter(|(control, first_end)| {
                first_end.is_some_and(|end| end <= before)
                    || prefix_has_bounded_control_suffix(prefix, control)
            })
            .count();
        (1usize.saturating_add(preservation_controls)) % 2 == 1
    }
}

fn prefix_has_bounded_control_suffix(prefix: &str, control: &str) -> bool {
    let prefix = prefix.trim_end();
    let Some(start) = prefix.len().checked_sub(control.len()) else {
        return false;
    };
    prefix[start..] == *control
        && prefix[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !word_continuation(character))
}

fn secret_action_polarities(value: &str) -> Vec<(usize, bool)> {
    let mut polarities = vec![None; value.len().saturating_add(1)];
    for marker in SECRET_ACTIONS {
        #[cfg(test)]
        SECRET_ACTION_POLARITY_WORK.with(|work| {
            work.set(work.get().saturating_add(value.len().saturating_add(1)));
        });
        for (start, matched) in value.match_indices(marker) {
            let end = start.saturating_add(matched.len());
            if !marker_has_boundaries(value, start, end) {
                continue;
            }
            let polarity = !marker_is_negated(value, start, end);
            polarities[start] = Some(polarity);
            #[cfg(test)]
            SECRET_ACTION_POLARITY_WORK.with(|work| {
                work.set(work.get().saturating_add(1));
            });
        }
    }
    #[cfg(test)]
    SECRET_ACTION_POLARITY_WORK.with(|work| {
        work.set(work.get().saturating_add(polarities.len()));
    });
    polarities
        .into_iter()
        .enumerate()
        .filter_map(|(start, polarity)| polarity.map(|polarity| (start, polarity)))
        .collect()
}

fn maximal_secret_target_occurrences(value: &str) -> Vec<(usize, usize)> {
    let mut furthest_end_at_start = vec![None; value.len().saturating_add(1)];
    for marker in SECRET_TARGETS {
        #[cfg(test)]
        MAXIMAL_SECRET_TARGET_WORK.with(|work| {
            work.set(work.get().saturating_add(value.len().saturating_add(1)));
        });
        for (start, matched) in value.match_indices(marker) {
            let end = start.saturating_add(matched.len());
            if !marker_has_boundaries(value, start, end) {
                continue;
            }
            let slot = &mut furthest_end_at_start[start];
            *slot = Some(slot.map_or(end, |current: usize| current.max(end)));
            #[cfg(test)]
            MAXIMAL_SECRET_TARGET_WORK.with(|work| {
                work.set(work.get().saturating_add(1));
            });
        }
    }
    let mut maximal = Vec::new();
    let mut max_end = 0usize;
    for (start, end) in furthest_end_at_start.into_iter().enumerate() {
        #[cfg(test)]
        MAXIMAL_SECRET_TARGET_WORK.with(|work| work.set(work.get().saturating_add(1)));
        let Some(end) = end else {
            continue;
        };
        if end <= max_end {
            continue;
        }
        max_end = end;
        maximal.push((start, end));
    }
    maximal
}

fn has_unsafe_secret_target(value: &str) -> bool {
    maximal_secret_target_occurrences(value)
        .into_iter()
        .any(|(start, end)| !secret_target_is_locally_safe(value, start, end))
}

pub(super) fn secret_target_is_locally_safe(value: &str, start: usize, end: usize) -> bool {
    if secret_target_has_value_reopener(&value[end..]) {
        return false;
    }
    let preceding = value[..start].split_whitespace().next_back();
    if preceding.is_some_and(|word| {
        matches!(
            word,
            "masked"
                | "redacted"
                | "replaced"
                | "substituted"
                | "가린"
                | "가려진"
                | "마스킹된"
                | "대체된"
                | "치환된"
                | "숨긴"
                | "숨겨진"
        )
    }) {
        return true;
    }
    if secret_target_is_metadata(value, start, end) {
        return true;
    }
    let suffix = value[end..].trim_start();
    [
        "is masked",
        "is redacted",
        "is replaced",
        "is substituted",
        "remains masked",
        "remains redacted",
    ]
    .iter()
    .any(|predicate| {
        suffix.strip_prefix(predicate).is_some_and(|remainder| {
            remainder
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

fn secret_target_is_metadata(value: &str, start: usize, end: usize) -> bool {
    if secret_target_has_value_reopener(&value[end..]) {
        return false;
    }
    let prefix = value[..start].trim_end();
    if ["number of", "count of"]
        .iter()
        .any(|carrier| prefix.ends_with(carrier))
    {
        return true;
    }
    let prefix_words = prefix.split_whitespace().collect::<Vec<_>>();
    if prefix_words.len() >= 4
        && prefix_words[prefix_words.len().saturating_sub(4)..]
            .iter()
            .copied()
            .eq(["four", "characters", "of", "an"])
        && prefix_words
            .get(prefix_words.len().saturating_sub(5))
            .is_some_and(|word| *word == "last")
    {
        return true;
    }
    let suffix = value[end..].trim_start();
    [
        "configuration status",
        "expiry date",
        "expiration date",
        "fingerprint",
        "format",
        "health",
        "identifier",
        "is active",
        "is configured",
        "metadata",
        "policy",
        "requirements",
        "rotation status",
        "status",
        "usage count",
        "usage counts",
        "usage metric",
        "usage metrics",
    ]
    .iter()
    .any(|role| {
        suffix.strip_prefix(role).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

fn secret_target_has_value_reopener(suffix: &str) -> bool {
    let suffix = suffix.trim_start();
    if bounded_marker_occurrences(
        suffix,
        &[
            "and actual value",
            "and content",
            "and its actual value",
            "and its raw value",
            "and its value",
            "and raw value",
            "and secret content",
            "and their values",
            "and value",
            "together with its actual value",
            "together with its raw value",
            "together with its value",
            "with its actual value",
            "with its raw value",
            "with its value",
        ],
    )
    .next()
    .is_some()
    {
        return true;
    }
    bounded_marker_occurrences(suffix, SECRET_ACTIONS)
        .any(|(_, end)| starts_with_secret_value_reference(suffix[end..].trim_start()))
}

fn starts_with_secret_value_reference(value: &str) -> bool {
    [
        "actual value",
        "content",
        "its actual value",
        "its raw value",
        "its value",
        "raw value",
        "secret content",
        "the value",
        "their values",
        "value",
        "values",
    ]
    .iter()
    .any(|reference| {
        value.strip_prefix(reference).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

pub(super) fn starts_with_secret_target_object(value: &str) -> bool {
    maximal_secret_target_occurrences(value)
        .into_iter()
        .any(|(start, _)| {
            value[..start].split_whitespace().all(|word| {
                matches!(
                    word,
                    "a" | "all"
                        | "an"
                        | "any"
                        | "each"
                        | "every"
                        | "masked"
                        | "raw"
                        | "real"
                        | "redacted"
                        | "the"
                        | "unmasked"
                        | "unredacted"
                )
            })
        })
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

fn has_unnegated_boundary_action(value: &str, kind: BoundaryKind) -> bool {
    let (markers, base, third_person, passive) = match kind {
        BoundaryKind::Live => (
            LIVE_ACTIONS,
            LIVE_BASE_ACTIONS,
            LIVE_THIRD_PERSON_ACTIONS,
            LIVE_PASSIVE_ACTIONS,
        ),
        BoundaryKind::Secret => (
            SECRET_ACTIONS,
            SECRET_BASE_ACTIONS,
            SECRET_THIRD_PERSON_ACTIONS,
            SECRET_PASSIVE_ACTIONS,
        ),
        BoundaryKind::Gate => return false,
    };
    markers.iter().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            marker_has_boundaries(value, start, start + matched.len())
                && !marker_is_negated(value, start, start + matched.len())
                && boundary_action_is_authoritative(value, start)
                && closed_boundary_action_form(
                    value,
                    start,
                    marker,
                    kind,
                    base,
                    third_person,
                    passive,
                )
        })
    })
}

fn boundary_action_is_authoritative(value: &str, action_start: usize) -> bool {
    let prefix = value[..action_start].trim_end();
    ![
        "simulate",
        "simulates",
        "simulated",
        "simulating",
        "simulation of",
    ]
    .iter()
    .any(|carrier| prefix.ends_with(carrier))
}

fn closed_boundary_action_form(
    value: &str,
    start: usize,
    marker: &str,
    kind: BoundaryKind,
    base: &[&str],
    third_person: &[&str],
    passive: &[&str],
) -> bool {
    let end = start.saturating_add(marker.len());
    if !marker.is_ascii() && !closed_korean_boundary_action(value, start, end, marker, kind) {
        return false;
    }
    let typed =
        base.contains(&marker) || third_person.contains(&marker) || passive.contains(&marker);
    !typed
        || (base.contains(&marker) && closed_base_boundary_action(value, start, kind))
        || (third_person.contains(&marker)
            && closed_third_person_boundary_actor(value, start, kind))
        || (passive.contains(&marker) && closed_passive_boundary_action(value, start))
}

fn closed_korean_boundary_action(
    value: &str,
    _start: usize,
    end: usize,
    marker: &str,
    kind: BoundaryKind,
) -> bool {
    if kind == BoundaryKind::Secret && marker == "공개" {
        let suffix = &value[end..];
        if suffix.starts_with(char::is_whitespace)
            && ["채널", "패널", "메시지", "응답", "서버"]
                .iter()
                .any(|target| suffix.trim_start().starts_with(target))
        {
            return false;
        }
    }
    true
}

fn closed_third_person_boundary_actor(
    value: &str,
    action_start: usize,
    kind: BoundaryKind,
) -> bool {
    let prefix = value[..action_start].trim_end();
    let mut words = prefix.split_whitespace().rev();
    let mut actor = words.next();
    let mut adverbs = 0usize;
    while adverbs < 2 && actor.is_some_and(closed_boundary_action_adverb) {
        actor = words.next();
        adverbs = adverbs.saturating_add(1);
    }
    if actor == Some("that") {
        actor = words.next();
    }
    actor.is_some_and(|actor| closed_third_person_actor(kind, actor))
        || (kind == BoundaryKind::Secret
            && closed_public_secret_disclosure_subject(value, action_start))
}

fn closed_third_person_actor(kind: BoundaryKind, actor: &str) -> bool {
    CLOSED_THIRD_PERSON_BOUNDARY_ACTORS.contains(&actor)
        || (kind == BoundaryKind::Secret && CLOSED_SECRET_DISCLOSURE_ACTORS.contains(&actor))
}

pub(super) fn closed_boundary_action_adverb(word: &str) -> bool {
    CLOSED_BOUNDARY_ACTION_ADVERBS.contains(&word)
        || (word.len() > 2
            && word.ends_with("ly")
            && word.bytes().all(|byte| byte.is_ascii_alphabetic()))
}

fn closed_base_boundary_action(value: &str, action_start: usize, kind: BoundaryKind) -> bool {
    let prefix = value[..action_start].trim_end();
    if prefix.is_empty() {
        return true;
    }
    if kind == BoundaryKind::Live && closed_first_person_live_request(prefix) {
        return true;
    }
    if [
        "carry out",
        "do this:",
        "execute",
        "execute:",
        "perform",
        "please carry out",
    ]
    .iter()
    .any(|wrapper| prefix.ends_with(wrapper))
    {
        return true;
    }
    if kind == BoundaryKind::Live
        && ["make a", "make the"]
            .iter()
            .any(|wrapper| prefix.ends_with(wrapper))
    {
        return true;
    }
    let mut words = prefix.split_whitespace().rev();
    let mut head = words.next();
    while head.is_some_and(closed_boundary_action_adverb) {
        head = words.next();
    }
    let Some(head) = head else {
        return true;
    };
    if matches!(
        head,
        "can" | "could" | "may" | "might" | "must" | "please" | "should" | "to" | "will" | "would"
    ) || closed_third_person_actor(kind, head)
    {
        return true;
    }
    if head == "do" {
        return true;
    }
    if matches!(head, "does" | "did") {
        return words
            .next()
            .is_some_and(|actor| closed_third_person_actor(kind, actor));
    }
    head == "you"
        && words.next().is_some_and(|word| {
            matches!(
                word,
                "can" | "could" | "may" | "might" | "should" | "will" | "would"
            )
        })
}

fn closed_first_person_live_request(prefix: &str) -> bool {
    let mut words = prefix.split_whitespace().collect::<Vec<_>>();
    while words
        .last()
        .is_some_and(|word| closed_boundary_action_adverb(word))
    {
        words.pop();
    }
    matches!(words.as_slice(), ["let's" | "let’s"] | ["let", "us"])
}

fn closed_passive_boundary_action(value: &str, action_start: usize) -> bool {
    let prefix = value[..action_start].trim_end();
    let mut words = prefix.split_whitespace().rev();
    let mut head = words.next();
    let mut adverbs = 0usize;
    while adverbs < 2 && head.is_some_and(closed_boundary_action_adverb) {
        head = words.next();
        adverbs = adverbs.saturating_add(1);
    }
    let Some(head) = head else {
        return false;
    };
    if matches!(
        head,
        "am" | "are"
            | "be"
            | "been"
            | "being"
            | "get"
            | "gets"
            | "got"
            | "gotten"
            | "is"
            | "was"
            | "were"
    ) {
        return true;
    }
    false
}

pub(super) fn has_negated_gate_action_marker(value: &str) -> bool {
    closed_gate_control_meaning(value) == Some(SafetyControlMeaning::PreservesControl)
        || korean_safety_control_clause(value)
            == Some(KoreanSafetyControlClause::Control(
                SafetyControlMeaning::PreservesControl,
            ))
}

fn without_control_meaning(value: &str) -> Option<SafetyControlMeaning> {
    let words = value.split_whitespace().collect::<Vec<_>>();
    if let Some(meaning) = closed_without_safety_control_meaning(&words) {
        return Some(meaning);
    }
    let without = words.iter().position(|word| *word == "without")?;
    without_complement_weakens_control(&words[without.saturating_add(1)..]).map(|weakens| {
        if weakens {
            SafetyControlMeaning::WeakensControl
        } else {
            SafetyControlMeaning::PreservesControl
        }
    })
}

fn without_complement_weakens_control(words: &[&str]) -> Option<bool> {
    if closed_safety_control_tail(words) {
        return Some(true);
    }
    let action = safety_control_action(words)?;
    closed_safety_control_tail(&words[action.length..]).then_some(match action.effect {
        SafetyControlActionEffect::WeakensControl => false,
        SafetyControlActionEffect::EnforcesControl => true,
    })
}

pub(super) fn has_negated_boundary_action_marker(value: &str, kind: BoundaryKind) -> bool {
    match kind {
        BoundaryKind::Gate => has_negated_gate_action_marker(value),
        BoundaryKind::Live => marker_sets_have_negated_action(value, &[LIVE_ACTIONS]),
        BoundaryKind::Secret => marker_sets_have_negated_action(value, &[SECRET_ACTIONS]),
    }
}

fn marker_sets_have_negated_action(value: &str, marker_sets: &[&[&str]]) -> bool {
    marker_sets.iter().copied().flatten().any(|marker| {
        value.match_indices(marker).any(|(start, matched)| {
            marker_has_boundaries(value, start, start + matched.len())
                && marker_is_negated(value, start, start + matched.len())
        })
    })
}

pub(super) fn boundary_action_is_effectively_preserved(value: &str, kind: BoundaryKind) -> bool {
    has_negated_boundary_action_marker(value, kind)
        || (kind == BoundaryKind::Gate
            && closed_gate_control_meaning(value) == Some(SafetyControlMeaning::PreservesControl))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreservationContinuation {
    Gerund,
    Infinitive,
}

fn direct_preservation_continuation(
    value: &str,
    kind: BoundaryKind,
) -> Option<PreservationContinuation> {
    if kind != BoundaryKind::Gate
        || closed_gate_control_meaning(value) != Some(SafetyControlMeaning::PreservesControl)
    {
        return None;
    }
    let continuation = boundary_action_markers(kind).find_map(|marker| {
        value.match_indices(marker).find_map(|(start, matched)| {
            marker_has_boundaries(value, start, start + matched.len())
                .then(|| &value[..start])
                .filter(|prefix| {
                    preservation_prefix_directly_governs(prefix) && prefix_negates_action(prefix)
                })
                .map(|prefix| {
                    if prefix.trim_end().ends_with("refuse to") {
                        PreservationContinuation::Infinitive
                    } else {
                        PreservationContinuation::Gerund
                    }
                })
        })
    })?;
    Some(continuation)
}

fn starts_with_preservation_action(
    value: &str,
    kind: BoundaryKind,
    continuation: PreservationContinuation,
) -> bool {
    let mut value = value;
    while let Some(stripped) = ACTION_NEGATION_MODIFIERS.iter().find_map(|modifier| {
        value.strip_prefix(modifier).and_then(|suffix| {
            suffix
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
                .then_some(suffix.trim_start())
        })
    }) {
        value = stripped;
    }
    let words = value.split_whitespace().collect::<Vec<_>>();
    if kind == BoundaryKind::Gate {
        return safety_control_action(&words).is_some_and(|action| {
            action.matches_gerund(continuation == PreservationContinuation::Gerund)
        });
    }
    boundary_action_markers(kind)
        .filter_map(|marker| marker.split_whitespace().next())
        .filter(|marker| {
            marker.ends_with("ing") == (continuation == PreservationContinuation::Gerund)
        })
        .any(|marker| {
            value.strip_prefix(marker).is_some_and(|suffix| {
                suffix
                    .chars()
                    .next()
                    .is_none_or(|character| !word_continuation(character))
            })
        })
}

fn boundary_action_markers(kind: BoundaryKind) -> impl Iterator<Item = &'static str> + Clone {
    const GATE: &[&[&str]] = &[
        GATE_ACTIONS,
        GATE_DESTRUCTIVE_ACTIONS,
        GATE_EXACT_ACTIONS,
        GATE_REQUIREMENT_REVERSAL_ACTIONS,
    ];
    const LIVE: &[&[&str]] = &[LIVE_ACTIONS];
    const SECRET: &[&[&str]] = &[SECRET_ACTIONS];
    let marker_sets: &'static [&'static [&'static str]] = match kind {
        BoundaryKind::Gate => GATE,
        BoundaryKind::Live => LIVE,
        BoundaryKind::Secret => SECRET,
    };
    marker_sets.iter().copied().flatten().copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StructuralTail {
    Direct,
    Permitted,
    Prohibited,
}

fn closed_gate_control_meaning(value: &str) -> Option<SafetyControlMeaning> {
    closed_gate_control_meaning_mode(value, true)
}

fn closed_gate_control_meaning_mode(
    value: &str,
    allow_embedded_action: bool,
) -> Option<SafetyControlMeaning> {
    let core = strip_exact_prefix_wrappers(value.trim());
    if let Some((base, exception)) = split_gate_exception(core) {
        if closed_gate_control_meaning_mode(base, allow_embedded_action)
            == Some(SafetyControlMeaning::PreservesControl)
            && closed_gate_exception_scope(exception)
        {
            return Some(SafetyControlMeaning::WeakensControl);
        }
        return None;
    }
    if let Some((base, scope)) = split_restrictive_gate_scope(core) {
        if closed_gate_control_meaning_mode(base, allow_embedded_action)
            == Some(SafetyControlMeaning::PreservesControl)
            && closed_restricted_actor_scope(scope)
        {
            return Some(SafetyControlMeaning::WeakensControl);
        }
        return None;
    }
    let core = strip_exact_suffix_wrappers(core);
    let words = core.split_whitespace().collect::<Vec<_>>();
    if let Some(meaning) = closed_safety_control_result_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_actor_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_active_actor_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_passive_target_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_configuration_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_safety_control_state_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_without_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_inverted_subject_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_subject_safety_control_meaning(&words) {
        return Some(meaning);
    }
    if let Some(meaning) = closed_root_safety_control_action_meaning(&words) {
        return Some(meaning);
    }
    if !allow_embedded_action {
        return None;
    }
    if let Some(meaning) = without_control_meaning(core) {
        return Some(meaning);
    }
    let without_complement =
        words
            .iter()
            .rposition(|word| *word == "without")
            .and_then(|without| {
                without_complement_weakens_control(&words[without.saturating_add(1)..])
                    .map(|_| without.saturating_add(1))
            });
    (0..words.len()).find_map(|index| {
        if without_complement.is_some_and(|start| index >= start)
            || (safety_control_action(&words[index..]).is_none()
                && !words[index..]
                    .first()
                    .is_some_and(|word| matches!(*word, "turn" | "turning" | "turns")))
        {
            return None;
        }
        let negated = action_prefix_polarity_words(&words[..index]).0;
        closed_safety_control_action_meaning(&words[index..], negated)
            .or_else(|| closed_separable_turn_off_safety_control_meaning(&words[index..], negated))
    })
}

fn closed_root_safety_control_action_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    for (index, word) in words.iter().enumerate() {
        if safety_control_action_head(word) {
            #[cfg(test)]
            ROOT_SAFETY_CONTROL_ACTION_PROBES
                .with(|steps| steps.set(steps.get().saturating_add(1)));
            let prefix = &words[..index];
            let negated = action_prefix_polarity_words(prefix).0;
            if let Some(meaning) = closed_safety_control_action_meaning(&words[index..], negated)
                .or_else(|| {
                    closed_separable_turn_off_safety_control_meaning(&words[index..], negated)
                })
            {
                return Some(meaning);
            }
        }
        #[cfg(test)]
        ROOT_SAFETY_CONTROL_PREFIX_STEPS.with(|steps| steps.set(steps.get().saturating_add(1)));
        if !closed_safety_control_action_prefix_word(word) {
            break;
        }
    }
    None
}

fn safety_control_action_head(word: &str) -> bool {
    matches!(word, "turn" | "turning" | "turns") || safety_control_action(&[word]).is_some()
}

fn closed_safety_control_action_prefix_word(word: &str) -> bool {
    ACTION_NEGATION_MODIFIERS.contains(&word)
        || closed_preservation_actor_term(word)
        || PRESERVATION_DETERMINERS.contains(&word)
        || matches!(word, "action" | "actions" | "all" | "every" | "from")
        || ORDINARY_PREFIX_NEGATIONS
            .iter()
            .chain(PRESERVATION_PREFIX_NEGATIONS)
            .flat_map(|control| control.split_whitespace())
            .any(|term| term == word)
}

fn split_gate_exception(value: &str) -> Option<(&str, &str)> {
    [" unless ", " except for ", " except "]
        .iter()
        .find_map(|connector| value.split_once(connector))
}

fn split_restrictive_gate_scope(value: &str) -> Option<(&str, &str)> {
    if let Some(parts) = value.split_once(" only for ") {
        return Some(parts);
    }
    let value = value.strip_suffix(" only")?;
    value.rsplit_once(" for ")
}

fn closed_restricted_actor_scope(value: &str) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    (1..=3).contains(&words.len())
        && words.iter().all(|word| {
            matches!(
                *word,
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "member"
                    | "members"
                    | "owner"
                    | "owners"
                    | "the"
                    | "user"
                    | "users"
            )
        })
        && words.iter().any(|word| !matches!(*word, "the"))
}

fn closed_gate_exception_scope(value: &str) -> bool {
    let words = value.split_whitespace().collect::<Vec<_>>();
    (1..=8).contains(&words.len())
        && words.iter().all(|word| {
            matches!(
                *word,
                "a" | "action"
                    | "actions"
                    | "admin"
                    | "admins"
                    | "an"
                    | "approves"
                    | "by"
                    | "disables"
                    | "for"
                    | "from"
                    | "guest"
                    | "guests"
                    | "is"
                    | "it"
                    | "member"
                    | "members"
                    | "owner"
                    | "owners"
                    | "out"
                    | "opts"
                    | "request"
                    | "requests"
                    | "the"
                    | "user"
                    | "users"
                    | "waived"
            )
        })
        && words.iter().any(|word| {
            matches!(
                *word,
                "admin"
                    | "admins"
                    | "guest"
                    | "guests"
                    | "member"
                    | "members"
                    | "owner"
                    | "owners"
                    | "user"
                    | "users"
            )
        })
}

pub(super) fn closed_gate_control_weakening(value: &str) -> bool {
    closed_gate_control_meaning_mode(value, false) == Some(SafetyControlMeaning::WeakensControl)
        || korean_safety_control_clause(value)
            == Some(KoreanSafetyControlClause::Control(
                SafetyControlMeaning::WeakensControl,
            ))
}

fn korean_safety_control_clause(value: &str) -> Option<KoreanSafetyControlClause> {
    let words = value.split_whitespace().collect::<Vec<_>>();
    closed_korean_safety_control_clause(&words)
}

fn has_optional_gate_bypass(words: &[&str]) -> bool {
    words.iter().enumerate().any(|(index, action)| {
        if *action != "make" {
            return false;
        }
        let remainder = strip_safety_control_target_modifiers(&words[index.saturating_add(1)..]);
        let Some(target_length) = safety_control_target_length(remainder) else {
            return false;
        };
        let remainder = &remainder[target_length..];
        remainder.first() == Some(&"optional")
            && closed_structural_remainder(&remainder[1..]).is_some()
            && !action_prefix_polarity_words(&words[..index]).0
    })
}

fn has_optional_gate_bypass_text(value: &str) -> bool {
    let core = strip_exact_suffix_wrappers(strip_exact_prefix_wrappers(value.trim()));
    has_optional_gate_bypass(&core.split_whitespace().collect::<Vec<_>>())
}

fn closed_structural_remainder(words: &[&str]) -> Option<StructuralTail> {
    let words = strip_exact_tail_wrappers(words);
    if words.is_empty() || closed_safety_control_scope(words) {
        return Some(StructuralTail::Direct);
    }
    if let Some(length) = preservation_prohibition_length(words) {
        let remainder = &words[length..];
        if remainder.is_empty() || closed_safety_control_scope(remainder) {
            return Some(StructuralTail::Prohibited);
        }
    }
    if let Some(length) = action_permission_length(words) {
        let remainder = &words[length..];
        if remainder.is_empty() || closed_safety_control_scope(remainder) {
            return Some(StructuralTail::Permitted);
        }
    }
    None
}

fn strip_exact_tail_wrappers<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    loop {
        let Some(length) = [
            &["right", "away"][..],
            &["right", "now"][..],
            &["immediately"][..],
            &["please"][..],
            &["only"][..],
            &["now"][..],
        ]
        .iter()
        .find_map(|wrapper| words.starts_with(wrapper).then_some(wrapper.len())) else {
            return words;
        };
        words = &words[length..];
    }
}

fn strip_exact_prefix_wrappers(mut value: &str) -> &str {
    loop {
        let Some(stripped) = GATE_EXACT_PREFIX_WRAPPERS
            .iter()
            .find_map(|wrapper| value.strip_prefix(wrapper))
        else {
            return value;
        };
        value = stripped.trim_start();
    }
}

fn strip_exact_suffix_wrappers(mut value: &str) -> &str {
    loop {
        let Some(stripped) = GATE_EXACT_SUFFIX_WRAPPERS
            .iter()
            .find_map(|wrapper| value.strip_suffix(wrapper))
        else {
            return value;
        };
        value = stripped.trim_end();
    }
}

fn has_bounded_gate_target(value: &str) -> bool {
    GATE_TARGETS.iter().any(|target| {
        value.match_indices(target).any(|(start, matched)| {
            marker_has_boundaries(value, start, start.saturating_add(matched.len()))
        })
    })
}

fn marker_has_boundaries(value: &str, start: usize, end: usize) -> bool {
    let left = value[..start].chars().next_back();
    let right = value[end..].chars().next();
    let left_valid = !left.is_some_and(word_continuation);
    let right_valid =
        !right.is_some_and(word_continuation) || known_korean_marker_suffix(&value[end..]);
    left_valid && right_valid
}

fn known_korean_marker_suffix(value: &str) -> bool {
    [
        "가",
        "게",
        "고",
        "과",
        "기",
        "도",
        "된",
        "돼",
        "들",
        "를",
        "만",
        "면",
        "로",
        "를",
        "에",
        "에서",
        "에게",
        "와",
        "으",
        "은",
        "을",
        "의",
        "이",
        "인",
        "지",
        "주",
        "줘",
        "주세요",
        "하",
        "해",
    ]
    .iter()
    .any(|suffix| value.starts_with(suffix))
}

fn marker_is_negated(value: &str, start: usize, end: usize) -> bool {
    let prefix = &value[..start];
    let suffix = following_chars(value, end, 32);
    prefix_negates_action(prefix) || suffix_negates_action(&suffix)
}

pub(super) fn prefix_negates_action(prefix: &str) -> bool {
    action_prefix_polarity(prefix).0
}

fn preservation_prefix_directly_governs(prefix: &str) -> bool {
    action_prefix_polarity(prefix).1
}

fn action_prefix_polarity(prefix: &str) -> (bool, bool) {
    let mut words = prefix
        .split_whitespace()
        .rev()
        .take(ACTION_POLARITY_TOKEN_WINDOW)
        .collect::<Vec<_>>();
    words.reverse();
    action_prefix_polarity_words(&words)
}

fn action_prefix_polarity_words(words: &[&str]) -> (bool, bool) {
    let words = &words[words.len().saturating_sub(ACTION_POLARITY_TOKEN_WINDOW)..];
    let mut end = words.len();
    let mut controls = 0usize;
    let mut closest_preservation = false;
    loop {
        if end >= 2
            && words[end.saturating_sub(2)] == "not"
            && matches!(words[end.saturating_sub(1)], "just" | "only")
        {
            end = end.saturating_sub(2);
            continue;
        }
        while end > 0
            && (ACTION_NEGATION_MODIFIERS.contains(&words[end - 1])
                || closed_boundary_action_adverb(words[end - 1])
                || matches!(
                    words[end - 1],
                    "be" | "been" | "being" | "get" | "gets" | "got" | "gotten"
                ))
        {
            end = end.saturating_sub(1);
        }
        let matched = trailing_preservation_object_frame(words, end)
            .map(|start| (start, true))
            .or_else(|| trailing_passive_preservation_frame(words, end).map(|start| (start, true)))
            .or_else(|| trailing_negative_allow_frame(words, end).map(|start| (start, false)))
            .or_else(|| trailing_negative_actor_modal(words, end).map(|start| (start, false)))
            .or_else(|| {
                trailing_control(words, end, PRESERVATION_PREFIX_NEGATIONS)
                    .map(|start| (start, true))
            })
            .or_else(|| {
                trailing_control(words, end, ORDINARY_PREFIX_NEGATIONS).map(|start| (start, false))
            });
        let Some((start, preservation)) = matched else {
            break;
        };
        if controls == 0 {
            closest_preservation = preservation;
        }
        controls = controls.saturating_add(1);
        end = start;
    }
    (controls % 2 == 1, closest_preservation)
}

fn trailing_passive_preservation_frame(words: &[&str], end: usize) -> Option<usize> {
    let predicate = end.checked_sub(2)?;
    (words[end.saturating_sub(1)] == "from"
        && matches!(
            words[predicate],
            "blocked" | "disallowed" | "forbidden" | "prevented" | "prohibited" | "stopped"
        )
        && closed_passive_actor_start(words, predicate).is_some())
    .then_some(predicate)
}

fn trailing_negative_allow_frame(words: &[&str], end: usize) -> Option<usize> {
    if end < 3 || words[end.saturating_sub(1)] != "to" {
        return None;
    }
    let allow = (0..end.saturating_sub(1)).rev().find(|index| {
        matches!(
            words[*index],
            "allow" | "allowed" | "allowing" | "allows" | "permit" | "permits" | "permitting"
        )
    })?;
    let object = &words[allow.saturating_add(1)..end.saturating_sub(1)];
    let actor_is_closed = if object.is_empty() {
        words[allow] == "allowed" && closed_passive_actor_start(words, allow).is_some()
    } else {
        closed_actor_terms(object)
    };
    if !actor_is_closed {
        return None;
    }
    action_prefix_polarity_words(&words[..allow]).0.then_some(0)
}

fn closed_passive_actor_start(words: &[&str], predicate: usize) -> Option<usize> {
    let mut auxiliary_end = predicate;
    while auxiliary_end > 0
        && (ACTION_NEGATION_MODIFIERS.contains(&words[auxiliary_end - 1])
            || ORDINARY_PREFIX_NEGATIONS
                .iter()
                .flat_map(|control| control.split_whitespace())
                .any(|term| term == words[auxiliary_end - 1]))
    {
        auxiliary_end = auxiliary_end.saturating_sub(1);
    }
    let auxiliary = auxiliary_end.checked_sub(1)?;
    if !matches!(
        words[auxiliary],
        "am" | "are"
            | "be"
            | "been"
            | "being"
            | "get"
            | "gets"
            | "got"
            | "gotten"
            | "is"
            | "was"
            | "were"
    ) {
        return None;
    }
    (1..=3).rev().find_map(|length| {
        let start = auxiliary.checked_sub(length)?;
        closed_actor_terms(&words[start..auxiliary]).then_some(start)
    })
}

fn closed_actor_terms(words: &[&str]) -> bool {
    let words = if words
        .first()
        .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word))
    {
        &words[1..]
    } else {
        words
    };
    !words.is_empty()
        && words.len() <= 2
        && words
            .iter()
            .all(|word| closed_preservation_actor_term(word))
}

fn closed_preservation_actor_term(word: &str) -> bool {
    PRESERVATION_ACTOR_TERMS.contains(&word) || CLOSED_THIRD_PERSON_BOUNDARY_ACTORS.contains(&word)
}

fn trailing_negative_actor_modal(words: &[&str], end: usize) -> Option<usize> {
    let modal = end.checked_sub(1)?;
    if !matches!(
        words[modal],
        "can" | "could" | "may" | "might" | "must" | "should" | "will" | "would"
    ) {
        return None;
    }
    if modal >= 1 && matches!(words[modal - 1], "nobody" | "none") {
        return Some(modal - 1);
    }
    if modal >= 2 && words[modal - 2] == "no" && words[modal - 1] == "one" {
        return Some(modal - 2);
    }
    if modal >= 2 && words[modal - 2] == "no" && closed_preservation_actor_term(words[modal - 1]) {
        Some(modal - 2)
    } else {
        None
    }
}

fn trailing_control(words: &[&str], end: usize, controls: &[&str]) -> Option<usize> {
    controls.iter().find_map(|control| {
        let length = control.split_whitespace().count();
        let start = end.checked_sub(length)?;
        words[start..end]
            .iter()
            .copied()
            .eq(control.split_whitespace())
            .then_some(start)
    })
}

fn trailing_preservation_object_frame(words: &[&str], end: usize) -> Option<usize> {
    if end < 3 || words[end - 1] != "from" {
        return None;
    }
    let object_end = end.saturating_sub(1);
    for object_length in 1..=3 {
        let Some(object_start) = object_end.checked_sub(object_length) else {
            continue;
        };
        let objects = &words[object_start..object_end];
        let objects = if objects
            .first()
            .is_some_and(|word| PRESERVATION_DETERMINERS.contains(word))
        {
            &objects[1..]
        } else {
            objects
        };
        if objects.is_empty()
            || objects.len() > 2
            || (!objects
                .iter()
                .all(|word| closed_preservation_actor_term(word))
                && !closed_boundary_preservation_object(objects))
        {
            continue;
        }
        if let Some(start) = trailing_control(words, object_start, PRESERVATION_PREFIX_NEGATIONS) {
            return Some(start);
        }
    }
    None
}

fn closed_boundary_preservation_object(words: &[&str]) -> bool {
    let value = words.join(" ");
    contains_bounded_any(&value, SECRET_TARGETS)
        || matches!(value.as_str(), "live changes" | "production changes")
}

pub(super) fn suffix_negates_action(suffix: &str) -> bool {
    let suffix = suffix.trim_start();
    if korean_suffix_negates_action(suffix).is_some_and(|negated| negated) {
        return true;
    }
    SUFFIX_NEGATIONS.iter().any(|negation| {
        suffix.strip_prefix(negation).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

fn korean_suffix_negates_action(value: &str) -> Option<bool> {
    let remainder = [
        "되지 않게",
        "되지 못하게",
        "지 않게",
        "지 못하게",
        "를 막",
        "을 막",
        "를 금지",
        "을 금지",
    ]
    .iter()
    .find_map(|marker| value.strip_prefix(marker))?;
    let preservation_negated = ["지 말", "지마", "하지 마", "하지마"]
        .iter()
        .any(|marker| remainder.contains(marker));
    Some(!preservation_negated)
}

fn following_chars(value: &str, start: usize, limit: usize) -> String {
    value[start..].chars().take(limit).collect()
}

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn secret_target_collection_measures_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let value = "api token ".repeat(repeated);
            MAXIMAL_SECRET_TARGET_WORK.with(|steps| steps.set(0));
            assert_eq!(maximal_secret_target_occurrences(&value).len(), repeated);
            MAXIMAL_SECRET_TARGET_WORK.with(Cell::get)
        }

        let small = work(2_048);
        let large = work(4_096);
        assert!(large <= small.saturating_mul(2).saturating_add(SECRET_TARGETS.len()));
    }

    #[test]
    fn secret_action_polarities_measure_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let value = "publish api token ".repeat(repeated);
            SECRET_ACTION_POLARITY_WORK.with(|steps| steps.set(0));
            assert_eq!(secret_action_polarities(&value).len(), repeated);
            SECRET_ACTION_POLARITY_WORK.with(Cell::get)
        }

        let small = work(2_048);
        let large = work(4_096);
        assert!(large <= small.saturating_mul(2).saturating_add(SECRET_ACTIONS.len()));
    }

    #[test]
    fn unprotected_secret_control_queries_do_constant_prefix_work() {
        let repeated = 4_096usize;
        let value = format!("do not keep {}", "unmasked ".repeat(repeated));
        UNPROTECTED_SECRET_PREFIX_STEPS.with(|steps| steps.set(0));

        assert!(!has_unnegated_unprotected_secret(&value));
        let steps = UNPROTECTED_SECRET_PREFIX_STEPS.with(Cell::get);

        assert_eq!(
            steps,
            repeated.saturating_mul(1usize.saturating_add(PRESERVATION_PREFIX_NEGATIONS.len()))
        );
    }

    #[test]
    fn root_safety_control_scan_validates_each_prefix_word_once() {
        let repeated = 8_192usize;
        let mut words = vec!["also"; repeated];
        words.extend(["skip", "approval"]);
        ROOT_SAFETY_CONTROL_PREFIX_STEPS.with(|steps| steps.set(0));
        ROOT_SAFETY_CONTROL_ACTION_PROBES.with(|steps| steps.set(0));

        assert_eq!(
            closed_root_safety_control_action_meaning(&words),
            Some(SafetyControlMeaning::WeakensControl)
        );
        let prefix_steps = ROOT_SAFETY_CONTROL_PREFIX_STEPS.with(Cell::get);
        let action_probes = ROOT_SAFETY_CONTROL_ACTION_PROBES.with(Cell::get);

        assert_eq!(prefix_steps, repeated);
        assert_eq!(action_probes, 1);
    }
}
