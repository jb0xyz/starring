use super::intent_boundary_grounding::UnquotedGroundingLink;
use super::intent_interpretation::{
    EconomyRequirementV2, PersistenceRequirementV2, RuntimeRequirementsV2, TimerRequirementV2,
};
use super::intent_request_mode_grounding::GroundedSemanticUnit;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static TERM_OCCURRENCE_SCANS: Cell<usize> = const { Cell::new(0) };
}

const SETUP_ONLY_LLM_CONTEXTS: &[&str] = &[
    "llm only during setup",
    "llm during setup only",
    "llm at setup time",
    "llm only during design",
    "llm only at design time",
    "llm only during compilation",
    "llm only at compile time",
    "llm only before deployment",
    "language model only during setup",
    "language model during setup only",
    "language model at setup time",
    "ai를 설정할 때만",
    "ai를 설정 시점에만",
    "llm을 설정할 때만",
    "llm을 설정 시점에만",
    "언어 모델을 설정할 때만",
    "언어 모델을 설정 시점에만",
];
const SETUP_CONTEXT_BARRIERS: &[&str] = &[
    "generate initialization copy",
    "generate setup copy",
    "generate the setup copy",
    "llm during setup",
    "llm for setup",
    "language model during setup",
    "language model for setup",
    "llm before deployment",
    "llm while setting up",
    "language model before deployment",
    "language model while setting up",
];
const SETUP_TIME_SUFFIXES: &[&str] = &[
    "at compile time",
    "at design time",
    "at initialization time",
    "at setup time",
    "before deployment",
    "during compilation",
    "during design",
    "during initialization",
    "during setup",
    "for setup",
    "in the setup phase",
    "once during initialization",
    "while setting up",
    "배포 전에",
    "설계 시점에",
    "설계할 때",
    "설정 단계",
    "설정 시점에",
    "설정할 때",
    "초기 설정 때",
    "컴파일 시점에",
    "컴파일할 때",
];
const SETUP_ARTIFACT_QUALIFIERS: &[&str] = &[
    " compiled at setup time",
    " compiled during setup",
    " created at setup time",
    " created during setup",
    " generated at setup time",
    " generated during setup",
    " prepared at setup time",
    " prepared during setup",
];
const RESTART_MARKERS: &[&str] = &[
    "restart",
    "restarts",
    "reboot",
    "reboots",
    "재시작",
    "재부팅",
];
const SURVIVAL_MARKERS: &[&str] = &[
    "persist",
    "persistent",
    "preserve",
    "keep",
    "retain",
    "restore",
    "survive",
    "without losing",
    "must not lose",
    "restart-safe",
    "restart safe",
    "영속",
    "유지",
    "보존",
    "복구",
    "살아",
    "잃지",
];
const PERSISTENCE_SUBJECTS: &[&str] = &[
    "state",
    "data",
    "progress",
    "configuration",
    "records",
    "상태",
    "데이터",
    "진행",
    "설정",
    "기록",
];
const PERSISTENCE_DIRECT: &[&str] = &[
    "restart_persistent",
    "restart-persistent",
    "restart persistent",
    "persist state",
    "재시작 후에도 상태를 유지",
    "재시작해도 상태를 유지",
    "재시작 후에도 데이터를 유지",
    "재시작해도 데이터를 유지",
    "재시작 후에도 진행 상태를 보존",
    "재시작해도 진행 상태를 보존",
    "재시작 후에도 잃지 않",
    "재시작해도 잃지 않",
];
const PERSISTENT_STATE_MARKERS: &[&str] = &[
    "persistent state",
    "restart persistence",
    "state persistence",
    "영속 상태",
    "재시작 영속성",
];
const POSITIVE_REQUIREMENT_ACTIONS: &[&str] = &[
    "add", "enable", "include", "need", "require", "store", "use", "using", "with", "추가", "포함",
    "사용", "유지", "저장", "필요",
];
const POSITIVE_REQUIREMENT_SUFFIXES: &[&str] = &[
    "added", "enabled", "included", "required", "stored", "used", "추가", "포함", "사용", "저장",
    "진행", "유지", "필요",
];
const POSITIVE_PERSISTENCE_ACTIONS: &[&str] = &[
    "keep",
    "keeps",
    "persist",
    "persists",
    "preserve",
    "preserves",
    "restore",
    "restores",
    "retain",
    "retains",
    "store",
    "stores",
    "survive",
    "survives",
    "복구",
    "보존",
    "살아",
    "영속",
    "유지",
    "저장",
];
const PERSISTENCE_NEGATIONS: &[&str] = &[
    "do not persist state",
    "don't persist state",
    "do not persist data",
    "don't persist data",
    "do not preserve state",
    "don't preserve state",
    "do not preserve data",
    "don't preserve data",
    "do not retain state",
    "don't retain state",
    "do not restore state",
    "don't restore state",
    "state does not need to persist",
    "data does not need to persist",
    "state need not persist",
    "data need not persist",
    "do not use persistent state",
    "don't use persistent state",
    "without persistent state",
    "no persistent state",
    "without restart persistence",
    "no restart persistence",
    "persistent state is not required",
    "restart persistence is not required",
    "상태를 영속하지",
    "상태를 보존하지",
    "영속 상태를 사용하지",
    "영속 상태를 쓰지",
    "영속 상태 없이",
    "재시작 후 유지하지",
    "재시작해도 유지하지",
    "재시작 후에도 상태를 유지하지",
    "재시작해도 상태를 유지하지",
    "재시작 후에도 상태를 보존하지",
    "재시작해도 상태를 보존하지",
    "재시작 영속성 없이",
    "재시작 영속성은 필요 없",
];
const NEGATED_PERSISTENCE_ACTIONS: &[&str] = &[
    "can't persist",
    "can't preserve",
    "can't retain",
    "can't restore",
    "can't survive",
    "cannot persist",
    "cannot preserve",
    "cannot retain",
    "cannot restore",
    "cannot survive",
    "can’t persist",
    "can’t preserve",
    "can’t retain",
    "can’t restore",
    "can’t survive",
    "can never persist",
    "can never preserve",
    "can never retain",
    "can never restore",
    "can never survive",
    "must never persist",
    "must never preserve",
    "must never retain",
    "must never restore",
    "must never survive",
    "must not persist",
    "must not preserve",
    "must not retain",
    "must not restore",
    "must not survive",
    "should not persist",
    "should not preserve",
    "should not retain",
    "should not restore",
    "should not survive",
    "shouldn't persist",
    "shouldn't preserve",
    "shouldn't retain",
    "shouldn't restore",
    "shouldn't survive",
    "shouldn’t persist",
    "shouldn’t preserve",
    "shouldn’t retain",
    "shouldn’t restore",
    "shouldn’t survive",
    "should never persist",
    "should never preserve",
    "should never retain",
    "should never restore",
    "should never survive",
    "does not need to persist",
    "does not need to survive",
    "does not persist",
    "does not preserve",
    "does not retain",
    "does not restore",
    "does not survive",
    "need not persist",
    "need not survive",
    "영속하지",
    "유지하지",
    "보존하지",
    "복구하지",
    "살아남지",
    "유지하면 안",
    "사용하지",
    "쓰지",
];

const TIMER_MARKERS: &[&str] = &[
    "timer",
    "timers",
    "scheduler",
    "schedulers",
    "scheduled job",
    "타이머",
    "스케줄러",
    "예약 작업",
];
const DURABLE_TIMER_PATTERNS: &[&str] = &[
    "durable timer",
    "durable timers",
    "persistent timer",
    "persistent timers",
    "durable scheduler",
    "durable schedulers",
    "persistent scheduler",
    "persistent schedulers",
    "restart-safe timer",
    "restart-safe timers",
    "restart safe timer",
    "restart safe timers",
    "timer must be durable",
    "timers must be durable",
    "timer must be persistent",
    "timers must be persistent",
    "scheduler must be durable",
    "schedulers must be durable",
    "scheduled job must be durable",
    "scheduled jobs must be durable",
    "timer survives restarts",
    "timers survive restarts",
    "timer must survive restarts",
    "timers must survive restarts",
    "quest timer must survive restarts",
    "quest timers must survive restarts",
    "영속 타이머",
    "내구성 타이머",
    "영속 스케줄러",
    "내구성 스케줄러",
    "재시작 후에도 유지되는 타이머",
    "재시작해도 유지되는 타이머",
    "타이머는 재시작 후에도 유지",
    "타이머가 재시작 후에도 유지",
    "타이머는 재시작해도 유지",
    "타이머가 재시작해도 유지",
];
const DURABLE_TIMER_MARKERS: &[&str] = &[
    "durable scheduler",
    "durable schedulers",
    "durable timer",
    "durable timers",
    "persistent scheduler",
    "persistent schedulers",
    "persistent timer",
    "persistent timers",
    "restart safe timer",
    "restart safe timers",
    "restart-safe timer",
    "restart-safe timers",
    "내구성 스케줄러",
    "내구성 타이머",
    "영속 스케줄러",
    "영속 타이머",
];
const DURABLE_TIMER_ASSERTIONS: &[&str] = &[
    "build durable timer",
    "build durable timers",
    "preserve durable timer",
    "preserve durable timers",
    "scheduled job must be durable",
    "scheduled jobs must be durable",
    "scheduler must be durable",
    "schedulers must be durable",
    "timer must be durable",
    "timer must be persistent",
    "timer must survive restarts",
    "timer survives restarts",
    "timers must be durable",
    "timers must be persistent",
    "timers must survive restarts",
    "timers survive restarts",
    "quest timer must survive restarts",
    "quest timers must survive restarts",
    "재시작 후에도 유지되는 타이머",
    "재시작해도 유지되는 타이머",
    "타이머가 재시작 후에도 유지",
    "타이머가 재시작해도 유지",
    "타이머는 재시작 후에도 유지",
    "타이머는 재시작해도 유지",
];
const TIMER_NEGATIONS: &[&str] = &[
    "without durable timer",
    "without durable timers",
    "without persistent timer",
    "without persistent timers",
    "no durable timer",
    "no durable timers",
    "no persistent timer",
    "no persistent timers",
    "do not use durable timer",
    "do not use durable timers",
    "don't use durable timer",
    "don't use durable timers",
    "durable timers are not required",
    "timer durability is not required",
    "timers do not need to be durable",
    "timer does not need to be durable",
    "타이머 영속성 없이",
    "영속 타이머 없이",
    "내구성 타이머 없이",
    "영속 타이머를 사용하지",
    "영속 타이머를 안 사용",
    "영속 타이머를 안 써",
    "내구성 타이머를 사용하지",
    "영속 타이머 말고",
    "영속 타이머는 쓰지 마",
    "영속 타이머는 쓰지마",
    "영속 타이머는 안 써",
    "내구성 타이머는 쓰지 마",
    "내구성 타이머는 쓰지마",
    "타이머는 영속적일 필요 없",
];
const NON_RUNTIME_TIMER_SURFACES: &[&str] = &[
    "timer button",
    "timer channel",
    "timer label",
    "timer message",
    "timer panel",
    "timer role",
];

const ECONOMY_PERSISTENCE: &[&str] = &[
    "persistent xp",
    "persistent experience points",
    "persistent economy",
    "persistent balance",
    "xp storage",
    "experience points storage",
    "economy storage",
    "reward storage",
    "balance storage",
    "xp database",
    "economy database",
    "reward database",
    "balance database",
    "xp must be persistent",
    "xp must persist",
    "experience points must be persistent",
    "experience points must persist",
    "economy must be persistent",
    "economy must persist",
    "economy ledger must be persistent",
    "economy ledger must persist",
    "rewards must be persistent",
    "rewards must persist",
    "balances must be persistent",
    "balances must persist",
    "xp survives restarts",
    "keep xp across restarts",
    "preserve xp across restarts",
    "retain xp across restarts",
    "economy survives restarts",
    "reward balances survive restarts",
    "xp is stored persistently",
    "economy is stored persistently",
    "rewards are stored persistently",
    "balances are stored persistently",
    "영속 경험치",
    "영속 경제",
    "영속 보상",
    "영속 잔액",
    "경험치 저장소",
    "경제 저장소",
    "보상 저장소",
    "잔액 저장소",
    "경제 원장을 영속",
    "경제 원장은 영속",
    "경제 원장이 영속",
    "경험치를 영구 저장",
    "보상을 영구 저장",
    "잔액을 영구 저장",
];
const ECONOMY_PERSISTENCE_MARKERS: &[&str] = &[
    "balance database",
    "balance storage",
    "economy database",
    "economy storage",
    "experience points storage",
    "persistent balance",
    "persistent economy",
    "persistent experience points",
    "persistent xp",
    "reward database",
    "reward storage",
    "xp database",
    "xp storage",
    "경제 저장소",
    "경험치 저장소",
    "보상 저장소",
    "영속 경제",
    "영속 경험치",
    "영속 보상",
    "영속 잔액",
    "잔액 저장소",
];
const ECONOMY_PERSISTENCE_ASSERTIONS: &[&str] = &[
    "balances are stored persistently",
    "balances must be persistent",
    "balances must persist",
    "economy is stored persistently",
    "economy ledger must be persistent",
    "economy ledger must persist",
    "economy must be persistent",
    "economy must persist",
    "economy survives restarts",
    "experience points must be persistent",
    "experience points must persist",
    "keep xp across restarts",
    "preserve xp across restarts",
    "retain xp across restarts",
    "reward balances survive restarts",
    "rewards are stored persistently",
    "rewards must be persistent",
    "rewards must persist",
    "xp is stored persistently",
    "xp must be persistent",
    "xp must persist",
    "xp survives restarts",
    "경제 원장을 영속",
    "경제 원장은 영속",
    "경제 원장이 영속",
    "경험치를 영구 저장",
    "보상을 영구 저장",
    "잔액을 영구 저장",
];
const ECONOMY_NEGATIONS: &[&str] = &[
    "without persistent xp",
    "without a persistent economy",
    "no persistent economy",
    "do not use a persistent economy",
    "do not use persistent economy",
    "don't use a persistent economy",
    "don't use persistent economy",
    "persistent economy is not required",
    "economy does not need to persist",
    "economy ledger does not need to be persistent",
    "영속 경제 없이",
    "영속 경제 말고",
    "영속 경제를 사용하지",
    "영속 경제를 안 사용",
    "영속 경제를 안 써",
    "경제 영속성 없이",
    "경제는 영속적일 필요 없",
];
const ECONOMY_STORAGE_SUBJECTS: &[&str] = &[
    "xp",
    "experience points",
    "economy",
    "reward",
    "rewards",
    "balance",
    "balances",
    "ledger",
    "경험치",
    "경제",
    "보상",
    "잔액",
    "원장",
];
const ECONOMY_STORAGE_NEGATING_ACTIONS: &[&str] = &[
    "do not persist",
    "do not store",
    "don't persist",
    "don't store",
    "don’t persist",
    "don’t store",
    "must not persist",
    "must not store",
    "never persist",
    "never store",
    "should not persist",
    "should not store",
    "without persisting",
    "without storing",
];

const LLM_MARKERS: &[&str] = &[
    "llm",
    "language model",
    "artificial intelligence",
    "ai",
    "ai가",
    "ai는",
    "언어 모델",
    "인공지능",
];
const NON_MODEL_LLM_SURFACES: &[&str] = &[
    " button", " channel", " label", " message", " modal", " panel", " role", " room", " 버튼",
    " 역할", " 채널", " 패널",
];
const LLM_ACTIONS: &[&str] = &[
    "decide",
    "decides",
    "choose",
    "chooses",
    "generate",
    "generates",
    "evaluate",
    "evaluates",
    "infer",
    "infers",
    "invoked",
    "execute",
    "executes",
    "run",
    "runs",
    "결정",
    "선택",
    "생성",
    "평가",
    "추론",
    "실행",
    "호출",
];
const LLM_PASSIVE_ACTIONS: &[&str] = &[
    "calling",
    "chosen by",
    "decided by",
    "evaluated by",
    "executed by",
    "generated by",
    "inferred by",
    "invoking",
    "run by",
];
const EVENT_TIME_MARKERS: &[&str] = &[
    "at event time",
    "at runtime",
    "during an event",
    "during events",
    "when an event",
    "when the event",
    "on every message",
    "for every message",
    "per event",
    "이벤트 시점",
    "실행 시점",
    "요청 시점",
    "이벤트가 발생",
    "이벤트마다",
    "메시지마다",
];
const EVENT_LLM_NEGATIONS: &[&str] = &[
    "without an llm",
    "without llm",
    "do not call an llm",
    "don't call an llm",
    "do not use an llm",
    "don't use an llm",
    "do not run an llm",
    "don't run an llm",
    "don’t call an llm",
    "don’t use an llm",
    "don’t run an llm",
    "must not call an llm",
    "never call an llm",
    "never let an llm",
    "never let the llm",
    "never ask an llm",
    "never ask the llm",
    "never run an llm",
    "never use an llm",
    "do not let an llm",
    "don't let an llm",
    "do not let the llm",
    "don't let the llm",
    "do not ask an llm",
    "don't ask an llm",
    "do not ask the llm",
    "don't ask the llm",
    "do not allow an llm",
    "do not allow the llm",
    "don't allow an llm",
    "don't allow the llm",
    "don’t allow an llm",
    "don’t allow the llm",
    "do not permit an llm",
    "do not permit the llm",
    "don't permit an llm",
    "don't permit the llm",
    "don’t permit an llm",
    "don’t permit the llm",
    "no event-time llm",
    "llm 없이",
    "언어 모델 없이",
    "인공지능 없이",
    "llm을 호출하지",
    "llm을 실행하지",
    "llm을 사용하지",
];
const DIRECT_EVENT_LLM_NEGATIONS: &[&str] = &[
    "no event-time llm",
    "no event time llm",
    "이벤트 시점 llm 없이",
    "이벤트 시점 언어 모델 없이",
    "이벤트 시점 인공지능 없이",
];
const NEGATED_LLM_ACTIONS: &[&str] = &[
    "must not decide",
    "must not choose",
    "must not generate",
    "must not evaluate",
    "must not infer",
    "must not execute",
    "must not run",
    "should not decide",
    "should not choose",
    "should not generate",
    "should not evaluate",
    "should not infer",
    "should not execute",
    "should not run",
    "does not decide",
    "does not choose",
    "does not generate",
    "does not evaluate",
    "does not infer",
    "does not execute",
    "does not run",
    "is forbidden to decide",
    "is forbidden to choose",
    "is forbidden to generate",
    "is forbidden to evaluate",
    "is forbidden to infer",
    "is forbidden to execute",
    "is forbidden to run",
    "are forbidden to decide",
    "are forbidden to choose",
    "are forbidden to generate",
    "are forbidden to evaluate",
    "are forbidden to infer",
    "are forbidden to execute",
    "are forbidden to run",
    "is not allowed to decide",
    "is not allowed to choose",
    "is not allowed to generate",
    "is not allowed to evaluate",
    "is not allowed to infer",
    "is not allowed to execute",
    "is not allowed to run",
    "are not allowed to decide",
    "are not allowed to choose",
    "are not allowed to generate",
    "are not allowed to evaluate",
    "are not allowed to infer",
    "are not allowed to execute",
    "are not allowed to run",
    "cannot decide",
    "cannot choose",
    "cannot generate",
    "cannot evaluate",
    "cannot infer",
    "cannot execute",
    "cannot run",
    "may not decide",
    "may not choose",
    "may not generate",
    "may not evaluate",
    "may not infer",
    "may not execute",
    "may not run",
    "never decide",
    "never choose",
    "never generate",
    "never evaluate",
    "never infer",
    "never execute",
    "never run",
    "결정하면 안",
    "결정해서는 안",
    "선택하면 안",
    "생성하면 안",
    "평가하면 안",
    "추론하면 안",
    "실행하면 안",
    "결정하지",
    "선택하지",
    "생성하지",
    "평가하지",
    "추론하지",
    "실행하지",
];
const NEGATED_REQUIREMENT_USE_ACTIONS: &[&str] = &[
    "must not be added",
    "must not be included",
    "must not be stored",
    "must not be used",
    "must not be enabled",
    "must not be required",
    "must never be used",
    "must never be enabled",
    "must never be required",
    "should not be added",
    "should not be included",
    "should not be stored",
    "should not be used",
    "should not be enabled",
    "should not be required",
    "is not required",
    "are not required",
    "is forbidden",
    "are forbidden",
    "is not allowed",
    "are not allowed",
    "is prohibited",
    "are prohibited",
    "is disallowed",
    "are disallowed",
    "isn't required",
    "isn’t required",
    "aren't required",
    "aren’t required",
    "does not need to be used",
    "do not need to be used",
    "not needed",
    "unnecessary",
    "optional",
    "사용하지",
    "사용하면 안",
    "쓰지",
    "쓰면 안",
    "필요 없",
    "필요하지",
    "선택 사항",
    "금지",
];

const NEGATING_ACTIONS: &[&str] = &[
    "avoid",
    "avoid using",
    "disable",
    "exclude",
    "omit",
    "remove",
    "do not add",
    "do not enable",
    "do not include",
    "do not let",
    "do not keep",
    "do not need",
    "do not persist",
    "do not preserve",
    "do not require",
    "do not restore",
    "do not retain",
    "do not use",
    "do not store",
    "without using",
    "cannot use",
    "can't use",
    "can’t use",
    "not use",
    "don't add",
    "don't enable",
    "don't include",
    "don't let",
    "don't keep",
    "don't need",
    "don't persist",
    "don't preserve",
    "don't require",
    "don't restore",
    "don't retain",
    "don't use",
    "don't store",
    "don’t add",
    "don’t enable",
    "don’t include",
    "don’t let",
    "don’t keep",
    "don’t need",
    "don’t persist",
    "don’t preserve",
    "don’t require",
    "don’t restore",
    "don’t retain",
    "don’t use",
    "don’t store",
    "use neither",
    "must not add",
    "must not enable",
    "must not include",
    "must not require",
    "must not use",
    "never add",
    "never enable",
    "never include",
    "never keep",
    "never persist",
    "never preserve",
    "never require",
    "never restore",
    "never retain",
    "never use",
    "no need for",
    "should not add",
    "should not enable",
    "should not include",
    "should not require",
    "should not use",
    "비활성화",
    "사용하지",
    "제거",
    "제외",
];

const MAXIMUM_PROXIMITY_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeRequirementAxis {
    Persistence,
    Timers,
    Economy,
    EventTimeLlm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RuntimeGroundingAmbiguity {
    Conflict,
    Alternative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RuntimeGroundingError {
    pub(super) axis: RuntimeRequirementAxis,
    pub(super) ambiguity: RuntimeGroundingAmbiguity,
}

#[derive(Default)]
struct AxisEvidence {
    positive: bool,
    negative: bool,
}

impl AxisEvidence {
    fn observe(&mut self, positive: bool, negative: bool) {
        if negative {
            self.negative = true;
        } else if positive {
            self.positive = true;
        }
    }

    fn required(&self, axis: RuntimeRequirementAxis) -> Result<bool, RuntimeGroundingError> {
        if self.positive && self.negative {
            return Err(RuntimeGroundingError {
                axis,
                ambiguity: RuntimeGroundingAmbiguity::Conflict,
            });
        }
        Ok(self.positive)
    }

    fn reject_prior_positive(&mut self) {
        if self.positive {
            self.positive = false;
            self.negative = true;
        }
    }
}

pub(crate) fn ground_runtime_requirements(
    active_semantic_units: &[GroundedSemanticUnit],
) -> Result<RuntimeRequirementsV2, RuntimeGroundingError> {
    let mut persistence = AxisEvidence::default();
    let mut timers = AxisEvidence::default();
    let mut economy = AxisEvidence::default();
    let mut event_time_llm = AxisEvidence::default();
    let mut previous: Option<&GroundedSemanticUnit> = None;
    let mut event_scope = false;
    for unit in active_semantic_units {
        if unit.link == UnquotedGroundingLink::Detached {
            event_scope = false;
        }
        if unit.link == UnquotedGroundingLink::Alternative
            || (unit.link == UnquotedGroundingLink::Additive && unit.text.starts_with("otherwise "))
        {
            if let Some(previous) = previous {
                if unit.authoritative || previous.authoritative {
                    validate_runtime_alternative(&previous.text, &unit.text, event_scope)?;
                }
            }
        }
        if !unit.authoritative {
            event_scope = false;
            previous = Some(unit);
            continue;
        }
        validate_inline_runtime_alternative(&unit.text)?;
        validate_inline_runtime_conflict(&unit.text)?;
        if bare_requirement_rejection(&unit.text) {
            if let Some(previous) = previous.filter(|previous| previous.authoritative) {
                for axis in positive_runtime_axes(&previous.text) {
                    match axis {
                        RuntimeRequirementAxis::Persistence => persistence.reject_prior_positive(),
                        RuntimeRequirementAxis::Timers => timers.reject_prior_positive(),
                        RuntimeRequirementAxis::Economy => economy.reject_prior_positive(),
                        RuntimeRequirementAxis::EventTimeLlm => {
                            event_time_llm.reject_prior_positive()
                        }
                    }
                }
            }
        }
        let inherited_event_scope = unit.link == UnquotedGroundingLink::Additive && event_scope;
        persistence.observe(
            persistence_required(&unit.text),
            persistence_rejected(&unit.text),
        );
        timers.observe(
            durable_timer_required(&unit.text),
            durable_timer_rejected(&unit.text),
        );
        economy.observe(
            persistent_economy_required(&unit.text),
            persistent_economy_rejected(&unit.text),
        );
        event_time_llm.observe(
            event_time_llm_required(&unit.text, inherited_event_scope),
            event_time_llm_rejected(&unit.text, inherited_event_scope),
        );
        let explicit_event_scope = has_any(&unit.text, EVENT_TIME_MARKERS);
        event_scope = match unit.link {
            UnquotedGroundingLink::Detached => explicit_event_scope,
            UnquotedGroundingLink::Additive => event_scope || explicit_event_scope,
            UnquotedGroundingLink::Alternative => false,
        } && !event_context_barrier(&unit.text);
        previous = Some(unit);
    }
    Ok(RuntimeRequirementsV2 {
        persistence: if persistence.required(RuntimeRequirementAxis::Persistence)? {
            PersistenceRequirementV2::RestartPersistent
        } else {
            PersistenceRequirementV2::None
        },
        timers: if timers.required(RuntimeRequirementAxis::Timers)? {
            TimerRequirementV2::Durable
        } else {
            TimerRequirementV2::None
        },
        economy: if economy.required(RuntimeRequirementAxis::Economy)? {
            EconomyRequirementV2::PersistentLedger
        } else {
            EconomyRequirementV2::None
        },
        event_time_llm: event_time_llm.required(RuntimeRequirementAxis::EventTimeLlm)?,
    })
}

fn validate_runtime_alternative(
    previous: &str,
    current: &str,
    inherited_event_scope: bool,
) -> Result<(), RuntimeGroundingError> {
    let mut previous_axes = positive_runtime_axes(previous);
    let mut current_axes = positive_runtime_axes(current);
    if inherited_event_scope
        && event_scoped_llm_action_required(previous)
        && !event_time_llm_rejected(previous, true)
    {
        previous_axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    if inherited_event_scope
        && event_scoped_llm_action_required(current)
        && !event_time_llm_rejected(current, true)
    {
        current_axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    if previous_axes.is_empty() {
        previous_axes = unrejected_runtime_mentions(previous);
    }
    if current_axes.is_empty() {
        current_axes = unrejected_runtime_mentions(current);
    }
    if previous_axes.is_empty() && current_axes.is_empty() {
        return Ok(());
    }
    let axis = previous_axes
        .iter()
        .chain(current_axes.iter())
        .copied()
        .min_by_key(runtime_axis_order);
    let Some(axis) = axis else {
        return Ok(());
    };
    Err(RuntimeGroundingError {
        axis,
        ambiguity: RuntimeGroundingAmbiguity::Alternative,
    })
}

fn validate_inline_runtime_alternative(text: &str) -> Result<(), RuntimeGroundingError> {
    if let Some((requirement, _)) = text.split_once(" unless ") {
        return unresolved_alternative(positive_runtime_axes(requirement));
    }
    for marker in [
        "choose between ",
        "pick between ",
        "select one from ",
        "one of ",
    ] {
        if let Some((_, tail)) = text.split_once(marker) {
            if runtime_choice_starts(tail) {
                return unresolved_alternative(runtime_axis_mentions(tail));
            }
        }
    }
    if [
        " 중 하나",
        "중 하나",
        " 중 택일",
        "중 택일",
        "든 하나",
        "골라",
        "otherwise ",
    ]
    .iter()
    .any(|marker| text.contains(marker))
    {
        return unresolved_alternative(runtime_axis_mentions(text));
    }
    if let Some((previous, current)) = ["and/or", " xor ", " versus "]
        .iter()
        .find_map(|marker| text.split_once(marker))
    {
        return unresolved_alternative(
            runtime_axis_mentions(previous)
                .into_iter()
                .chain(runtime_axis_mentions(current))
                .collect(),
        );
    }
    if let Some((previous, current)) = [" 또는 ", " 혹은 ", " 아니면 ", "거나 "]
        .iter()
        .find_map(|marker| text.split_once(marker))
    {
        let axes = positive_runtime_axes(previous)
            .into_iter()
            .chain(positive_runtime_axes(current))
            .chain(positive_runtime_axes(text))
            .collect();
        return unresolved_alternative(axes);
    }
    if let Some((previous, current)) = ["이나 ", "나 "]
        .iter()
        .find_map(|marker| text.split_once(marker))
    {
        if runtime_particle_precedes(previous) {
            return unresolved_alternative(
                runtime_axis_mentions(previous)
                    .into_iter()
                    .chain(runtime_axis_mentions(current))
                    .chain(positive_runtime_axes(text))
                    .collect(),
            );
        }
    }
    Ok(())
}

fn validate_inline_runtime_conflict(text: &str) -> Result<(), RuntimeGroundingError> {
    let rejected_axes = rejected_runtime_axes(text, false);
    for marker in [
        " without ",
        " but do not ",
        " but don't ",
        " but don’t ",
        " but never ",
    ] {
        let Some((positive, rejected)) = text.split_once(marker) else {
            continue;
        };
        if runtime_axis_mentions(rejected).is_empty() {
            continue;
        }
        if let Some(axis) = positive_runtime_axes(positive)
            .into_iter()
            .filter(|axis| rejected_axes.contains(axis))
            .min_by_key(runtime_axis_order)
        {
            return Err(RuntimeGroundingError {
                axis,
                ambiguity: RuntimeGroundingAmbiguity::Conflict,
            });
        }
    }
    Ok(())
}

fn runtime_choice_starts(tail: &str) -> bool {
    let tail = tail
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ':' | '-')
        })
        .strip_prefix("a ")
        .or_else(|| tail.trim_start().strip_prefix("an "))
        .or_else(|| tail.trim_start().strip_prefix("the "))
        .unwrap_or(tail.trim_start());
    [
        "durable scheduler",
        "durable timer",
        "persistent economy",
        "persistent scheduler",
        "persistent state",
        "persistent timer",
        "restart persistence",
        "내구성 타이머",
        "영속 경제",
        "영속 상태",
        "영속 타이머",
    ]
    .iter()
    .any(|marker| tail.starts_with(marker))
}

fn runtime_particle_precedes(previous: &str) -> bool {
    let previous = previous.trim_end();
    [
        "내구성 스케줄러",
        "내구성 타이머",
        "영속 경제",
        "영속 경험치",
        "영속 보상",
        "영속 상태",
        "영속 스케줄러",
        "영속 잔액",
        "영속 타이머",
        "재시작 영속성",
    ]
    .iter()
    .any(|marker| previous.ends_with(marker))
}

fn unresolved_alternative(axes: Vec<RuntimeRequirementAxis>) -> Result<(), RuntimeGroundingError> {
    let Some(axis) = axes.into_iter().min_by_key(runtime_axis_order) else {
        return Ok(());
    };
    Err(RuntimeGroundingError {
        axis,
        ambiguity: RuntimeGroundingAmbiguity::Alternative,
    })
}

fn bare_requirement_rejection(text: &str) -> bool {
    matches!(
        text.trim(),
        "not needed"
            | "not required"
            | "unnecessary"
            | "optional"
            | "they are not needed"
            | "they are not required"
            | "they are optional"
            | "필요 없어"
            | "필요 없습니다"
            | "선택 사항이야"
            | "선택 사항입니다"
    )
}

fn positive_runtime_axes(text: &str) -> Vec<RuntimeRequirementAxis> {
    let mut axes = Vec::new();
    if persistence_required(text) && !persistence_rejected(text) {
        axes.push(RuntimeRequirementAxis::Persistence);
    }
    if durable_timer_required(text) && !durable_timer_rejected(text) {
        axes.push(RuntimeRequirementAxis::Timers);
    }
    if persistent_economy_required(text) && !persistent_economy_rejected(text) {
        axes.push(RuntimeRequirementAxis::Economy);
    }
    if event_time_llm_required(text, false) && !event_time_llm_rejected(text, false) {
        axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    axes
}

fn rejected_runtime_axes(text: &str, inherited_event_scope: bool) -> Vec<RuntimeRequirementAxis> {
    let mut axes = Vec::new();
    if persistence_rejected(text) {
        axes.push(RuntimeRequirementAxis::Persistence);
    }
    if durable_timer_rejected(text) {
        axes.push(RuntimeRequirementAxis::Timers);
    }
    if persistent_economy_rejected(text) {
        axes.push(RuntimeRequirementAxis::Economy);
    }
    if event_time_llm_rejected(text, inherited_event_scope) {
        axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    axes
}

fn runtime_axis_mentions(text: &str) -> Vec<RuntimeRequirementAxis> {
    let mut axes = Vec::new();
    if has_any(text, PERSISTENT_STATE_MARKERS) || has_any(text, PERSISTENCE_DIRECT) {
        axes.push(RuntimeRequirementAxis::Persistence);
    }
    if has_any(text, DURABLE_TIMER_PATTERNS) {
        axes.push(RuntimeRequirementAxis::Timers);
    }
    if has_any(text, ECONOMY_PERSISTENCE) {
        axes.push(RuntimeRequirementAxis::Economy);
    }
    if has_any(text, EVENT_TIME_MARKERS)
        && has_runtime_llm_marker(text)
        && llm_runtime_action_required(text)
    {
        axes.push(RuntimeRequirementAxis::EventTimeLlm);
    }
    axes
}

fn unrejected_runtime_mentions(text: &str) -> Vec<RuntimeRequirementAxis> {
    runtime_axis_mentions(text)
        .into_iter()
        .filter(|axis| match axis {
            RuntimeRequirementAxis::Persistence => !persistence_rejected(text),
            RuntimeRequirementAxis::Timers => !durable_timer_rejected(text),
            RuntimeRequirementAxis::Economy => !persistent_economy_rejected(text),
            RuntimeRequirementAxis::EventTimeLlm => !event_time_llm_rejected(text, false),
        })
        .collect()
}

fn runtime_axis_order(axis: &RuntimeRequirementAxis) -> u8 {
    match axis {
        RuntimeRequirementAxis::Persistence => 0,
        RuntimeRequirementAxis::Timers => 1,
        RuntimeRequirementAxis::Economy => 2,
        RuntimeRequirementAxis::EventTimeLlm => 3,
    }
}

fn persistence_required(text: &str) -> bool {
    has_any(text, PERSISTENCE_DIRECT)
        || requirement_action_owns(text, PERSISTENT_STATE_MARKERS, 8)
        || ordered_near(
            text,
            PERSISTENT_STATE_MARKERS,
            POSITIVE_REQUIREMENT_SUFFIXES,
            8,
        )
        || (has_any(text, RESTART_MARKERS)
            && (ordered_near(text, POSITIVE_PERSISTENCE_ACTIONS, PERSISTENCE_SUBJECTS, 8)
                || ordered_near(text, PERSISTENCE_SUBJECTS, POSITIVE_PERSISTENCE_ACTIONS, 8)))
}

fn durable_timer_required(text: &str) -> bool {
    has_any(text, DURABLE_TIMER_ASSERTIONS)
        || requirement_action_owns(text, DURABLE_TIMER_MARKERS, 8)
        || ordered_near(
            text,
            DURABLE_TIMER_MARKERS,
            POSITIVE_REQUIREMENT_SUFFIXES,
            8,
        )
}

fn persistent_economy_required(text: &str) -> bool {
    has_any(text, ECONOMY_PERSISTENCE_ASSERTIONS)
        || requirement_action_owns(text, ECONOMY_PERSISTENCE_MARKERS, 8)
        || ordered_near(
            text,
            ECONOMY_PERSISTENCE_MARKERS,
            POSITIVE_REQUIREMENT_SUFFIXES,
            8,
        )
}

fn event_time_llm_required(text: &str, inherited_event_scope: bool) -> bool {
    (has_any(text, EVENT_TIME_MARKERS) || (inherited_event_scope && !event_context_barrier(text)))
        && event_scoped_llm_action_required(text)
}

fn event_scoped_llm_action_required(text: &str) -> bool {
    !setup_scoped_llm_action(text) && llm_runtime_action_required(text)
}

fn event_context_barrier(text: &str) -> bool {
    setup_scoped_llm_action(text)
        || has_any(text, SETUP_CONTEXT_BARRIERS)
        || [
            "during setup ",
            "during setup only ",
            "only during setup ",
            "at setup time ",
            "during design ",
            "only during design ",
            "at design time ",
            "during compilation ",
            "during initialization ",
            "only during compilation ",
            "at compile time ",
            "at initialization time ",
            "before deployment ",
            "only before deployment ",
            "before launch ",
            "for setup ",
            "in the setup phase ",
            "once during initialization ",
            "while setting up ",
            "설정할 때 ",
            "설정 단계",
            "초기 설정 때",
            "설정 시점에 ",
            "설계할 때 ",
            "설계 시점에 ",
            "컴파일할 때 ",
            "컴파일 시점에 ",
            "배포 전에 ",
        ]
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

fn setup_scoped_llm_action(text: &str) -> bool {
    has_any(text, SETUP_ONLY_LLM_CONTEXTS)
        || (llm_before_action_near(text, SETUP_TIME_SUFFIXES, 16)
            && !has_any(text, SETUP_ARTIFACT_QUALIFIERS))
}

fn llm_runtime_action_required(text: &str) -> bool {
    llm_before_action_near(text, LLM_ACTIONS, 8)
        || action_before_llm_near(text, LLM_PASSIVE_ACTIONS, 8)
        || has_any(
            text,
            &[
                "call an llm",
                "call the llm",
                "calls an llm",
                "calls the llm",
                "run an llm",
                "run the llm",
                "runs an llm",
                "runs the llm",
                "invoke an llm",
                "invoke the llm",
                "invokes an llm",
                "invokes the llm",
                "llm gets called",
                "llm is called",
                "the llm gets called",
                "the llm is called",
                "use an llm to",
                "use the llm to",
                "uses an llm to",
                "uses the llm to",
                "ask an llm to",
                "ask the llm to",
                "asks an llm to",
                "asks the llm to",
                "llm을 호출",
                "llm을 실행",
                "언어 모델을 호출",
                "언어 모델을 실행",
                "ai를 호출",
                "ai를 실행",
            ],
        )
}

fn persistence_rejected(text: &str) -> bool {
    has_any(text, PERSISTENCE_NEGATIONS)
        || (ordered_near(text, NEGATING_ACTIONS, PERSISTENCE_SUBJECTS, 8)
            && (has_any(text, RESTART_MARKERS) || has_any(text, SURVIVAL_MARKERS)))
        || (ordered_near(text, PERSISTENCE_SUBJECTS, NEGATED_PERSISTENCE_ACTIONS, 8)
            && has_any(text, RESTART_MARKERS))
        || (ordered_near(
            text,
            PERSISTENCE_SUBJECTS,
            NEGATED_REQUIREMENT_USE_ACTIONS,
            8,
        ) && (has_any(text, RESTART_MARKERS) || has_any(text, PERSISTENT_STATE_MARKERS)))
}

fn durable_timer_rejected(text: &str) -> bool {
    let non_runtime_surface = has_any(text, NON_RUNTIME_TIMER_SURFACES);
    has_any(text, TIMER_NEGATIONS)
        || ordered_near(text, NEGATING_ACTIONS, DURABLE_TIMER_MARKERS, 8)
        || (!non_runtime_surface
            && (ordered_near(text, NEGATING_ACTIONS, TIMER_MARKERS, 8)
                || ordered_near(text, TIMER_MARKERS, NEGATED_REQUIREMENT_USE_ACTIONS, 8)
                || ordered_near(text, TIMER_MARKERS, NEGATED_PERSISTENCE_ACTIONS, 8)))
}

fn persistent_economy_rejected(text: &str) -> bool {
    has_any(text, ECONOMY_NEGATIONS)
        || ordered_near(text, NEGATING_ACTIONS, ECONOMY_PERSISTENCE, 8)
        || ordered_near(
            text,
            ECONOMY_STORAGE_NEGATING_ACTIONS,
            ECONOMY_STORAGE_SUBJECTS,
            8,
        )
        || ordered_near(
            text,
            ECONOMY_PERSISTENCE,
            NEGATED_REQUIREMENT_USE_ACTIONS,
            8,
        )
        || ordered_near(
            text,
            ECONOMY_STORAGE_SUBJECTS,
            NEGATED_PERSISTENCE_ACTIONS,
            8,
        )
}

fn event_time_llm_rejected(text: &str, inherited_event_scope: bool) -> bool {
    let event_scope = has_any(text, EVENT_TIME_MARKERS)
        || (inherited_event_scope && !event_context_barrier(text));
    has_any(text, DIRECT_EVENT_LLM_NEGATIONS)
        || (event_scope
            && (has_any(text, EVENT_LLM_NEGATIONS)
                || action_before_llm_near(text, NEGATING_ACTIONS, 8)
                || llm_before_action_near(text, NEGATED_LLM_ACTIONS, 8)))
}

fn has_any(text: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| has_term(text, candidate))
}

fn requirement_action_owns(text: &str, target_subjects: &[&str], maximum_words: usize) -> bool {
    let all_subjects = PERSISTENT_STATE_MARKERS
        .iter()
        .chain(DURABLE_TIMER_MARKERS)
        .chain(ECONOMY_PERSISTENCE_MARKERS)
        .copied()
        .collect::<Vec<_>>();
    let next_target = next_term_start(text, target_subjects);
    let next_subject = next_term_start(text, &all_subjects);
    let missing = text.len().saturating_add(1);
    POSITIVE_REQUIREMENT_ACTIONS
        .iter()
        .flat_map(|action| term_occurrences(text, action))
        .any(|(_, action_end)| {
            let target = next_target[action_end];
            let nearest = next_subject[action_end];
            target != missing
                && target == nearest
                && target.saturating_sub(action_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[action_end..target].split_whitespace().count() <= maximum_words
        })
}

fn ordered_near(text: &str, subjects: &[&str], actions: &[&str], maximum_words: usize) -> bool {
    let next_action = next_term_start(text, actions);
    let missing = text.len().saturating_add(1);
    subjects.iter().any(|subject| {
        term_occurrences(text, subject).any(|(_, subject_end)| {
            let action_start = next_action[subject_end];
            action_start != missing
                && action_start.saturating_sub(subject_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[subject_end..action_start].split_whitespace().count() <= maximum_words
        })
    })
}

fn llm_before_action_near(text: &str, actions: &[&str], maximum_words: usize) -> bool {
    let next_action = next_term_start(text, actions);
    let missing = text.len().saturating_add(1);
    runtime_llm_occurrences(text)
        .into_iter()
        .any(|(_, subject_end)| {
            let action_start = next_action[subject_end];
            action_start != missing
                && action_start.saturating_sub(subject_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[subject_end..action_start].split_whitespace().count() <= maximum_words
        })
}

fn action_before_llm_near(text: &str, actions: &[&str], maximum_words: usize) -> bool {
    let missing = text.len().saturating_add(1);
    let next_llm = next_occurrence_start(
        text.len(),
        runtime_llm_occurrences(text)
            .into_iter()
            .map(|(start, _)| start),
    );
    actions.iter().any(|action| {
        term_occurrences(text, action).any(|(_, action_end)| {
            let llm_start = next_llm[action_end];
            llm_start != missing
                && llm_start.saturating_sub(action_end) <= MAXIMUM_PROXIMITY_BYTES
                && text[action_end..llm_start].split_whitespace().count() <= maximum_words
        })
    })
}

fn has_runtime_llm_marker(text: &str) -> bool {
    !runtime_llm_occurrences(text).is_empty()
}

fn runtime_llm_occurrences(text: &str) -> Vec<(usize, usize)> {
    LLM_MARKERS
        .iter()
        .flat_map(|marker| term_occurrences(text, marker))
        .filter(|(_, end)| {
            !NON_MODEL_LLM_SURFACES
                .iter()
                .any(|surface| text[*end..].starts_with(surface))
        })
        .collect()
}

fn next_term_start(text: &str, candidates: &[&str]) -> Vec<usize> {
    next_occurrence_start(
        text.len(),
        candidates
            .iter()
            .flat_map(|candidate| term_occurrences(text, candidate))
            .map(|(start, _)| start),
    )
}

fn next_occurrence_start(text_len: usize, starts: impl Iterator<Item = usize>) -> Vec<usize> {
    let missing = text_len.saturating_add(1);
    let mut next = vec![missing; text_len.saturating_add(1)];
    for start in starts {
        next[start] = start;
    }
    let mut nearest = missing;
    for index in (0..next.len()).rev() {
        if next[index] != missing {
            nearest = next[index];
        }
        next[index] = nearest;
    }
    next
}

#[cfg(test)]
pub(super) fn requirement_action_occurrence_scans(text: &str) -> usize {
    TERM_OCCURRENCE_SCANS.with(|scans| scans.set(0));
    let _ = requirement_action_owns(text, DURABLE_TIMER_MARKERS, 8);
    TERM_OCCURRENCE_SCANS.with(Cell::get)
}

fn term_occurrences<'a>(
    text: &'a str,
    candidate: &'a str,
) -> impl Iterator<Item = (usize, usize)> + 'a {
    #[cfg(test)]
    TERM_OCCURRENCE_SCANS.with(|scans| scans.set(scans.get().saturating_add(1)));
    text.match_indices(candidate).filter_map(move |(start, _)| {
        let end = start.saturating_add(candidate.len());
        let left = text[..start].chars().next_back();
        let right = text[end..].chars().next();
        let bounded = candidate.starts_with(' ')
            || candidate.ends_with(' ')
            || !candidate.is_ascii()
            || (!left.is_some_and(ascii_word_character)
                && !right.is_some_and(ascii_word_character));
        bounded.then_some((start, end))
    })
}

fn has_term(text: &str, candidate: &str) -> bool {
    if candidate.starts_with(' ') || candidate.ends_with(' ') || !candidate.is_ascii() {
        return text.contains(candidate);
    }
    text.match_indices(candidate).any(|(start, _)| {
        let end = start.saturating_add(candidate.len());
        let left = text[..start].chars().next_back();
        let right = text[end..].chars().next();
        !left.is_some_and(ascii_word_character) && !right.is_some_and(ascii_word_character)
    })
}

fn ascii_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
