#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RequestTarget {
    Automation,
    Bot,
    Button,
    Channel,
    Feature,
    Flow,
    Game,
    Modal,
    Panel,
    Role,
    Room,
    Rule,
    System,
    Workflow,
}

pub(super) const ENGLISH_BUILD_TARGETS: &[(&str, RequestTarget)] = &[
    ("automation", RequestTarget::Automation),
    ("bot", RequestTarget::Bot),
    ("button", RequestTarget::Button),
    ("channel", RequestTarget::Channel),
    ("feature", RequestTarget::Feature),
    ("flow", RequestTarget::Flow),
    ("game", RequestTarget::Game),
    ("modal", RequestTarget::Modal),
    ("panel", RequestTarget::Panel),
    ("role", RequestTarget::Role),
    ("room", RequestTarget::Room),
    ("rule", RequestTarget::Rule),
    ("rule set", RequestTarget::Rule),
    ("ruleset", RequestTarget::Rule),
    ("system", RequestTarget::System),
    ("workflow", RequestTarget::Workflow),
];

pub(super) const ENGLISH_BUILD_PREFIXES: &[&str] = &[
    "add ",
    "build ",
    "configure ",
    "create ",
    "design ",
    "implement ",
    "make ",
    "set up ",
];

pub(super) const ENGLISH_OBJECT_BARRIERS: &[&str] = &[
    " about ",
    " between ",
    " comparing ",
    " for ",
    " in ",
    " of ",
    " on ",
    " that ",
    " to ",
    " versus ",
    " vs ",
    " where ",
    " which ",
    " whose ",
    " with ",
    " using ",
];

pub(super) const ENGLISH_POLITE_BUILD_PREFIXES: &[&str] =
    &["can you ", "could you ", "will you ", "would you "];

pub(super) const ENGLISH_REQUEST_WRAPPERS: &[&str] = &[
    "actually ",
    "actually, ",
    "instead ",
    "now ",
    "now, ",
    "please ",
    "please, ",
];

pub(super) const ENGLISH_METALINGUISTIC_PREDICATES: &[&str] = &[
    " is a hypothetical request",
    " is an example prompt",
    " is an imperative example",
    " is an imperative phrase",
    " is only a hypothetical request",
    " is a hypothetical prompt",
    " would be a hypothetical request",
    " would only be a hypothetical request",
];

pub(super) const ENGLISH_DISCUSSION_DIRECTIVES: &[&str] = &[
    "brainstorming only",
    "brainstorming only for now",
    "discussion only",
    "discussion only for now",
    "just brainstorm",
    "just brainstorm for now",
    "let's only brainstorm",
    "let's only brainstorm for now",
    "let us only brainstorm",
    "let us only brainstorm for now",
    "only brainstorm",
    "only brainstorm for now",
];

pub(super) const ENGLISH_DRAFT_HOLD_DIRECTIVES: &[&str] = &[
    "do not change the draft",
    "do not change the draft for now",
    "do not change the draft yet",
    "don't change the draft",
    "don't change the draft for now",
    "don't change the draft yet",
    "don’t change the draft",
    "don’t change the draft for now",
    "don’t change the draft yet",
];

pub(super) const ENGLISH_BUILD_HOLD_PREFIXES: &[&str] = &[
    "do not add",
    "do not build",
    "do not configure",
    "do not create",
    "do not design",
    "do not implement",
    "do not make",
    "do not set up",
    "don't add",
    "don't build",
    "don't configure",
    "don't create",
    "don't design",
    "don't implement",
    "don't make",
    "don't set up",
    "don’t add",
    "don’t build",
    "don’t configure",
    "don’t create",
    "don’t design",
    "don’t implement",
    "don’t make",
    "don’t set up",
];

pub(super) const KOREAN_BUILD_SUFFIXES: &[&str] = &[
    "구축해줘",
    "구축해 줘",
    "구축해주세요",
    "구축해 주세요",
    "구축해 줄래",
    "구축해 줄래요",
    "구축해줄래",
    "구축해줄래요",
    "만들어줘",
    "만들어 줘",
    "만들어주세요",
    "만들어 주세요",
    "만들어 줄래",
    "만들어 줄래요",
    "만들어줄래",
    "만들어줄래요",
    "설계해줘",
    "설계해 줘",
    "설계해주세요",
    "설계해 주세요",
    "설계해 줄래",
    "설계해 줄래요",
    "설계해줄래",
    "설계해줄래요",
    "추가해줘",
    "추가해 줘",
    "추가해주세요",
    "추가해 주세요",
    "추가해 줄래",
    "추가해 줄래요",
    "추가해줄래",
    "추가해줄래요",
];

pub(super) const KOREAN_COMPOUND_BUILD_MARKERS: &[&str] =
    &["구축하고", "만들고", "설계하고", "추가하고"];

pub(super) const KOREAN_SPLIT_COMPOUND_BUILD_SUFFIXES: &[&str] = &["구축", "설계", "추가"];

pub(super) const KOREAN_COMPOUND_CONTINUATION_PREFIXES: &[&str] = &[
    "검증",
    "게시",
    "미리보기",
    "보여",
    "설정",
    "연결",
    "준비",
    "테스트",
    "확인",
];

pub(super) const KOREAN_BUILD_TARGETS: &[(&str, RequestTarget)] = &[
    ("자동화", RequestTarget::Automation),
    ("봇", RequestTarget::Bot),
    ("기능", RequestTarget::Feature),
    ("게임", RequestTarget::Game),
    ("규칙", RequestTarget::Rule),
    ("시스템", RequestTarget::System),
    ("워크플로", RequestTarget::Workflow),
    ("스터디룸", RequestTarget::Room),
    ("룸", RequestTarget::Room),
    ("패널", RequestTarget::Panel),
    ("역할", RequestTarget::Role),
    ("채널", RequestTarget::Channel),
    ("버튼", RequestTarget::Button),
    ("모달", RequestTarget::Modal),
    ("방", RequestTarget::Room),
];

pub(super) const KOREAN_TARGET_PARTICLES: &[&str] = &[
    "으로", "에서", "에게", "까지", "부터", "처럼", "보다", "조차", "마저", "은", "는", "이", "가",
    "을", "를", "에", "로", "과", "와", "도", "만", "의",
];

pub(super) const KOREAN_DISCUSSION_SUFFIXES: &[&str] = &[
    "논의만 하자",
    "논의만 해줘",
    "논의만 해주세요",
    "브레인스토밍만 하자",
    "브레인스토밍만 해줘",
    "브레인스토밍만 해주세요",
    "말지 논의하자",
    "말지를 논의하자",
    "여부를 논의하자",
];

pub(super) const KOREAN_NO_BUILD_SUFFIXES: &[&str] = &[
    "구축하지 말아줘",
    "구축하지 말아 줘",
    "구축하지 말아주세요",
    "구축하지 말아 주세요",
    "구축하지 말자",
    "구축하지 마",
    "구축하지마",
    "구축하지 마세요",
    "만들지는 말아줘",
    "만들지 말아줘",
    "만들지 말아 줘",
    "만들지 말아주세요",
    "만들지 말아 주세요",
    "만들지 말자",
    "만들지 마",
    "만들지마",
    "만들지 마세요",
    "설계하지 말아줘",
    "설계하지 말아 줘",
    "설계하지 말아주세요",
    "설계하지 말아 주세요",
    "설계하지 말자",
    "설계하지 마",
    "설계하지마",
    "설계하지 마세요",
    "추가하지 말아줘",
    "추가하지 말아 줘",
    "추가하지 말아주세요",
    "추가하지 말아 주세요",
    "추가하지 말자",
    "추가하지 마",
    "추가하지마",
    "추가하지 마세요",
];

pub(super) const KOREAN_BUILD_HOLD_MARKERS: &[&str] =
    &["당장은", "아직", "우선은", "일단은", "지금은"];

pub(super) const KOREAN_NEGATIVE_BUILD_VERBS: &[&str] =
    &["구축하지", "만들지는", "만들지", "설계하지", "추가하지"];

pub(super) const KOREAN_DRAFT_HOLD_DIRECTIVES: &[&str] =
    &["초안을 변경하지 마", "초안을 변경하지마"];
