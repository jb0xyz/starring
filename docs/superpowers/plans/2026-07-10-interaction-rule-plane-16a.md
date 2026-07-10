# Phase 16a — Interaction Rule Plane MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 이벤트-구동 선언형 자동화 엔진의 첫 primitive subset을 순수 코어(Mock)로 구현한다 — `ButtonClick` trigger → `GrantRole`/`RespondEphemeral` action을 결정론적으로 해석·실행.

**Architecture:** Layer 1과 병렬인 두 신규 크레이트. `automation-state`(선언형 rule 스키마, serde) + `automation-core`(validate/interpret/run + policy + seam trait + Mock). 런타임은 저장된 rule만 결정론적으로 해석하며 **LLM에 절대 의존하지 않는다**(빌드 의존성으로 강제). 실제 Discord gateway/interaction 연결은 Phase 16b.

**Tech Stack:** Rust (edition 2021, stable), serde(string-serde), bitflags 2, `futures::executor::block_on`(테스트). 신규 런타임 의존성 없음(tokio 없음).

## Global Constraints

- **코드 주석 절대 금지** — `//`, `///`, `//!` 전부. 플랜의 산문이 설명을 담고, 코드에는 주석을 넣지 않는다.
- **Codex가 구현한다** — 이 플랜의 코드 블록을 그대로 옮긴다. Claude는 코드를 작성하지 않는다.
- **edition/의존성은 workspace 상속** — `edition.workspace = true`, `serde = { workspace = true }`, 크레이트 간은 `path` 의존.
- **ID는 string-serde newtype** — `GuildId/RoleId/ChannelId/UserId(pub u64)`, Copy.
- **비동기 trait 패턴** — `#[allow(async_fn_in_trait)]` + native `async fn`. 제네릭 정적 디스패치(`&impl Trait`). tokio 금지, 테스트는 `block_on`.
- **automation 런타임 크레이트는 ai-gateway/LLM 의존 금지** — 이벤트-타임 AI 호출 금지의 빌드-레벨 강제. `automation-state`·`automation-core` **각 크레이트**가 자기 `tests/no_ai_gateway.rs`에서 자신의 `Cargo.toml`의 `ai-gateway` 문자열을 차단. (스펙 문장: "automation runtime crates must not depend on ai-gateway. Runtime must not call LLM during interaction handling.")
- **conditions 미지원 — 조용히 무시 금지.** 6개 스키마 타입 전부에 `#[serde(deny_unknown_fields)]`. `conditions`/`modal`/`template`/dynamic 등 미지원 필드가 오면 **역직렬화 실패**(무시 아님). serde 1.0.228은 internally-tagged enum(`tag = "type"`)에서도 `deny_unknown_fields`가 variant 내부 필드까지 거부함을 실증 확인. (스펙 문장: "Phase 16a does not support conditions. Unknown fields must be rejected, not ignored. If a rule contains conditions, deserialization or validation must fail.")
- **하나의 ButtonClick은 0개 또는 1개 rule에만 매칭** — 같은 button trigger를 쓰는 rule이 2개 이상이면 **validate 실패**(중복 실행보다 명확성 우선).
- **완료 게이트** — `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` 전부 통과. workspace `members`에 신규 크레이트 2개 추가. 완료 후 `git push origin main`.
- **live/토큰/DB/modal/dynamic-template 없음** — 전부 Phase 16b/후속.

---

## Scope 결정 (스펙 대비 확정 사항 — 사용자 승인 완료)

1. **`ConditionSpec`는 16a에서 제외.** 스펙 §6이 `conditions: Vec<ConditionSpec>`를 그렸지만, `ActorLacksRole` 평가에는 **actor의 현재 역할 상태**가 필요한데 MVP 이벤트 모델엔 그게 없다. 평가 못 하는 condition 필드를 넣으면 "gating을 기대했는데 무시되어 무조건 발동"하는 **silent non-gating 안전 구멍**이 생긴다. 그래서 16a `InteractionRule = { key, trigger, actions }`로 두고, condition은 member 상태가 생기는 16b에서 추가한다. (스펙 §5도 condition을 "선택, 없어도 됨"으로 명시.) **이 제외는 `deny_unknown_fields`로 강제** — conditions 필드가 오면 역직렬화가 실패하므로 조용히 무시되는 일이 없다.
2. **`AdapterError`는 automation-core 자체 정의**(executor-core 재사용 안 함). Layer 2를 Layer 1 executor 내부에 결합시키지 않기 위함(executor-core를 끌어오면 desired-compiler/diff-engine 등 무거운 transitive dep가 딸려옴). 작은 enum 복제 비용 < 레이어 디커플링 이득. 스펙 §14가 "또는 자체"를 허용.
3. **role key→id 해소는 `resource_resolution::ResourceBindingMap` 재사용.** 신규 registry 타입을 만들지 않는다. 16a 테스트는 이 바인딩 맵을 fixture로 주입(실제로는 Layer 1 해소 결과가 채운다).
4. **policy는 role 권한 맵을 입력으로 받는 순수 함수.** `analyze(ruleset, roles: &BTreeMap<ResourceKey, Permissions>)`. privileged 판정은 `discord_model::Permissions` 비트로 실제 디코딩. 권한 맵의 Layer 1 연결 배선은 후속.

---

## File Structure

**신규 크레이트 `automation-state`** (선언형 스키마):
- `crates/automation-state/Cargo.toml`
- `crates/automation-state/src/lib.rs` — 모듈 선언 + 재노출
- `crates/automation-state/src/rule.rs` — `InteractionRuleSet`, `InteractionRule`, `TriggerSpec`, `ActionSpec`, `ActionTarget`
- `crates/automation-state/src/panel.rs` — `PanelSpec`, `ButtonSpec`

**신규 크레이트 `automation-core`** (런타임 코어):
- `crates/automation-core/Cargo.toml`
- `crates/automation-core/src/lib.rs` — 모듈 선언 + 재노출
- `crates/automation-core/src/event.rs` — `RuntimeEvent`, `EventKind`, `RuntimeContext`
- `crates/automation-core/src/plan.rs` — `ActionPlan`, `PlannedAction`
- `crates/automation-core/src/adapter.rs` — `AdapterError`, `AdapterErrorKind`, `DiscordMutationAdapter`, `InteractionResponder`
- `crates/automation-core/src/mock.rs` — `MockMutationAdapter`, `MockInteractionResponder`, `MutationCall`, `ResponderCall`
- `crates/automation-core/src/validate.rs` — `ValidationError`, `validate`
- `crates/automation-core/src/interpret.rs` — `interpret`
- `crates/automation-core/src/run.rs` — `run`, `handle_event`
- `crates/automation-core/src/policy.rs` — `PolicyFinding`, `privileged_mask`, `analyze`
- `crates/automation-core/tests/validate.rs` — validate 시나리오
- `crates/automation-core/tests/interpret.rs` — interpret 시나리오
- `crates/automation-core/tests/run.rs` — run/handle_event 시나리오
- `crates/automation-core/tests/policy.rs` — policy 시나리오
- `crates/automation-core/tests/no_ai_gateway.rs` — 의존성 가드

**수정:**
- `Cargo.toml`(workspace root) — `members`에 두 크레이트 추가

---

## Task 1: `automation-state` 스키마 크레이트

**Files:**
- Create: `crates/automation-state/Cargo.toml`
- Create: `crates/automation-state/src/lib.rs`
- Create: `crates/automation-state/src/panel.rs`
- Create: `crates/automation-state/src/rule.rs`
- Create: `crates/automation-state/tests/no_ai_gateway.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Produces: `InteractionRuleSet { version: u32, panels: Vec<PanelSpec>, rules: Vec<InteractionRule> }`, `InteractionRule { key: String, trigger: TriggerSpec, actions: Vec<ActionSpec> }`, `TriggerSpec::ButtonClick { component: String }`, `ActionSpec::GrantRole { role: ResourceKey, target: ActionTarget }` / `ActionSpec::RespondEphemeral { content: String }`, `ActionTarget::Actor`, `PanelSpec { key, channel: ResourceKey, content, buttons: Vec<ButtonSpec> }`, `ButtonSpec { key, label }`. 전부 `Clone + Debug + PartialEq + Eq + Serialize + Deserialize`.

- [ ] **Step 1: workspace members에 automation-state 등록**

Modify `Cargo.toml` (root), `members` 배열에서 `"crates/bot-runtime",` 다음 줄에 추가. **automation-core는 여기서 등록하지 않는다** — member 경로에 manifest가 없으면 cargo가 워크스페이스 로드에 실패하므로, automation-core는 Task 2에서 자신의 Cargo.toml과 함께 등록한다.

```toml
    "crates/bot-runtime",
    "crates/automation-state",
```

- [ ] **Step 2: `crates/automation-state/Cargo.toml` 작성**

```toml
[package]
name = "automation-state"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
desired-state = { path = "../desired-state" }

[dev-dependencies]
serde_json = { workspace = true }
```

- [ ] **Step 3: `crates/automation-state/src/panel.rs` 작성**

```rust
use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    pub key: String,
    pub channel: ResourceKey,
    pub content: String,
    #[serde(default)]
    pub buttons: Vec<ButtonSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonSpec {
    pub key: String,
    pub label: String,
}
```

- [ ] **Step 4: `crates/automation-state/src/rule.rs` 작성**

```rust
use serde::{Deserialize, Serialize};

use desired_state::ResourceKey;

use crate::panel::PanelSpec;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionRuleSet {
    pub version: u32,
    #[serde(default)]
    pub panels: Vec<PanelSpec>,
    #[serde(default)]
    pub rules: Vec<InteractionRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionRule {
    pub key: String,
    pub trigger: TriggerSpec,
    pub actions: Vec<ActionSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerSpec {
    ButtonClick { component: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionSpec {
    GrantRole {
        role: ResourceKey,
        target: ActionTarget,
    },
    RespondEphemeral {
        content: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionTarget {
    Actor,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{ButtonSpec, PanelSpec};
    use desired_state::ResourceKey;

    fn sample() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![PanelSpec {
                key: "verify_panel".to_string(),
                channel: ResourceKey("verify_channel".to_string()),
                content: "click to verify".to_string(),
                buttons: vec![ButtonSpec {
                    key: "verify_button".to_string(),
                    label: "Verify".to_string(),
                }],
            }],
            rules: vec![InteractionRule {
                key: "verify_rule".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "verify_button".to_string(),
                },
                actions: vec![
                    ActionSpec::GrantRole {
                        role: ResourceKey("verified_member".to_string()),
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::RespondEphemeral {
                        content: "welcome".to_string(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn ruleset_json_roundtrips() {
        let set = sample();
        let json = serde_json::to_string(&set).unwrap();
        let back: InteractionRuleSet = serde_json::from_str(&json).unwrap();
        assert_eq!(set, back);
    }

    #[test]
    fn trigger_and_action_tagged_shape() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains(r#""type":"button_click""#));
        assert!(json.contains(r#""type":"grant_role""#));
        assert!(json.contains(r#""type":"respond_ephemeral""#));
        assert!(json.contains(r#""target":"actor""#));
    }

    #[test]
    fn panels_and_rules_default_when_absent() {
        let set: InteractionRuleSet = serde_json::from_str(r#"{"version":2}"#).unwrap();
        assert_eq!(set.version, 2);
        assert!(set.panels.is_empty());
        assert!(set.rules.is_empty());
    }

    #[test]
    fn conditions_field_is_rejected() {
        let json = r#"{"key":"r","trigger":{"type":"button_click","component":"b"},"conditions":[],"actions":[]}"#;
        assert!(serde_json::from_str::<InteractionRule>(json).is_err());
    }

    #[test]
    fn unknown_action_type_is_rejected() {
        let json = r#"{"type":"open_modal","modal":"m"}"#;
        assert!(serde_json::from_str::<ActionSpec>(json).is_err());
    }

    #[test]
    fn unknown_field_in_action_is_rejected() {
        let json = r#"{"type":"grant_role","role":"verified","target":"actor","template":"x"}"#;
        assert!(serde_json::from_str::<ActionSpec>(json).is_err());
    }
}
```

- [ ] **Step 5: `crates/automation-state/src/lib.rs` 작성**

```rust
pub mod panel;
pub mod rule;

pub use panel::{ButtonSpec, PanelSpec};
pub use rule::{ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, TriggerSpec};
```

- [ ] **Step 6: `crates/automation-state/tests/no_ai_gateway.rs` 작성** (런타임 크레이트 ai-gateway 의존 금지 가드)

```rust
#[test]
fn manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("ai-gateway"));
    assert!(!manifest.contains("ai_gateway"));
    assert!(!manifest.contains("llm"));
}
```

- [ ] **Step 7: 테스트 실행**

Run: `cargo test -p automation-state`
Expected: PASS (in-module 6 + no_ai_gateway 1 = 7 tests).

- [ ] **Step 8: 커밋**

```bash
git add crates/automation-state Cargo.toml
git commit -m "feat(automation-state): interaction rule schema crate with strict deserialization"
```

---

## Task 2: `automation-core` 토대 (event · plan · seam · mock · 의존성 가드)

**Files:**
- Modify: `Cargo.toml` (workspace members — automation-core 등록)
- Create: `crates/automation-core/Cargo.toml`
- Create: `crates/automation-core/src/lib.rs`
- Create: `crates/automation-core/src/event.rs`
- Create: `crates/automation-core/src/plan.rs`
- Create: `crates/automation-core/src/adapter.rs`
- Create: `crates/automation-core/src/mock.rs`
- Create: `crates/automation-core/tests/no_ai_gateway.rs`

**Interfaces:**
- Consumes: `discord_model::{GuildId, RoleId, UserId}`.
- Produces: `RuntimeEvent { guild_id, actor, kind }`, `EventKind::ButtonClick { component: String }`, `RuntimeContext { guild_id, actor }` + `RuntimeContext::from_event(&RuntimeEvent)`, `ActionPlan { steps: Vec<PlannedAction> }`, `PlannedAction::GrantRole { role: RoleId, target: UserId }` / `PlannedAction::RespondEphemeral { content: String }`, `AdapterError`/`AdapterErrorKind`, traits `DiscordMutationAdapter::grant_role(&self, GuildId, UserId, RoleId)` / `InteractionResponder::respond_ephemeral(&self, String)`, `MockMutationAdapter`/`MockInteractionResponder` (+ `.calls()`, `MutationCall`, `ResponderCall`).

- [ ] **Step 1: workspace members에 automation-core 등록 + `crates/automation-core/Cargo.toml` 작성**

먼저 root `Cargo.toml` `members`의 `"crates/automation-state",` 다음 줄에 `"crates/automation-core",`를 추가하고, 이어서 아래 manifest를 작성한다(이 둘은 함께 적용해야 cargo가 깨지지 않는다):

```toml
[package]
name = "automation-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
automation-state = { path = "../automation-state" }
desired-state = { path = "../desired-state" }
discord-model = { path = "../discord-model" }
resource-resolution = { path = "../resource-resolution" }

[dev-dependencies]
futures = "0.3"
serde_json = { workspace = true }
```

- [ ] **Step 2: `crates/automation-core/src/event.rs` 작성**

```rust
use discord_model::{GuildId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    ButtonClick { component: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeContext {
    pub guild_id: GuildId,
    pub actor: UserId,
}

impl RuntimeContext {
    pub fn from_event(event: &RuntimeEvent) -> Self {
        Self {
            guild_id: event.guild_id,
            actor: event.actor,
        }
    }
}
```

- [ ] **Step 3: `crates/automation-core/src/plan.rs` 작성**

```rust
use discord_model::{RoleId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPlan {
    pub steps: Vec<PlannedAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedAction {
    GrantRole { role: RoleId, target: UserId },
    RespondEphemeral { content: String },
}
```

- [ ] **Step 4: `crates/automation-core/src/adapter.rs` 작성**

```rust
use serde::{Deserialize, Serialize};

use discord_model::{GuildId, RoleId, UserId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKind {
    Forbidden,
    NotFound,
    RateLimited,
    Network,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterError {
    pub kind: AdapterErrorKind,
    pub message: String,
}

impl AdapterError {
    pub fn new(kind: AdapterErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait DiscordMutationAdapter {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError>;
}

#[allow(async_fn_in_trait)]
pub trait InteractionResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError>;
}
```

- [ ] **Step 5: `crates/automation-core/src/mock.rs` 작성**

```rust
use std::sync::Mutex;

use discord_model::{GuildId, RoleId, UserId};

use crate::adapter::{AdapterError, DiscordMutationAdapter, InteractionResponder};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationCall {
    GrantRole {
        guild: GuildId,
        member: UserId,
        role: RoleId,
    },
}

#[derive(Default)]
pub struct MockMutationAdapter {
    calls: Mutex<Vec<MutationCall>>,
    fail: Option<AdapterError>,
}

impl MockMutationAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn failing(error: AdapterError) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail: Some(error),
        }
    }

    pub fn calls(&self) -> Vec<MutationCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl DiscordMutationAdapter for MockMutationAdapter {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError> {
        self.calls.lock().unwrap().push(MutationCall::GrantRole {
            guild,
            member,
            role,
        });
        match &self.fail {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponderCall {
    RespondEphemeral { content: String },
}

#[derive(Default)]
pub struct MockInteractionResponder {
    calls: Mutex<Vec<ResponderCall>>,
}

impl MockInteractionResponder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calls(&self) -> Vec<ResponderCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl InteractionResponder for MockInteractionResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(ResponderCall::RespondEphemeral { content });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterErrorKind;
    use futures::executor::block_on;

    #[test]
    fn mutation_records_and_succeeds() {
        let mock = MockMutationAdapter::new();
        block_on(mock.grant_role(GuildId(1), UserId(42), RoleId(7))).unwrap();
        assert_eq!(
            mock.calls(),
            vec![MutationCall::GrantRole {
                guild: GuildId(1),
                member: UserId(42),
                role: RoleId(7),
            }]
        );
    }

    #[test]
    fn mutation_can_fail() {
        let mock = MockMutationAdapter::failing(AdapterError::new(AdapterErrorKind::Forbidden, "no"));
        let result = block_on(mock.grant_role(GuildId(1), UserId(42), RoleId(7)));
        assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
        assert_eq!(mock.calls().len(), 1);
    }

    #[test]
    fn responder_records() {
        let mock = MockInteractionResponder::new();
        block_on(mock.respond_ephemeral("hi".to_string())).unwrap();
        assert_eq!(
            mock.calls(),
            vec![ResponderCall::RespondEphemeral {
                content: "hi".to_string(),
            }]
        );
    }
}
```

- [ ] **Step 6: `crates/automation-core/src/lib.rs` 작성** (interpret/run/validate/policy는 이후 Task에서 채움 — 지금은 존재하는 모듈만 선언)

```rust
pub mod adapter;
pub mod event;
pub mod mock;
pub mod plan;

pub use adapter::{AdapterError, AdapterErrorKind, DiscordMutationAdapter, InteractionResponder};
pub use event::{EventKind, RuntimeContext, RuntimeEvent};
pub use mock::{MockInteractionResponder, MockMutationAdapter, MutationCall, ResponderCall};
pub use plan::{ActionPlan, PlannedAction};
```

- [ ] **Step 7: `crates/automation-core/tests/no_ai_gateway.rs` 작성** (이벤트-타임 AI 금지의 빌드-레벨 강제)

```rust
#[test]
fn core_manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("ai-gateway"),
        "automation-core must not depend on ai-gateway; event-time AI is forbidden"
    );
    assert!(!manifest.contains("ai_gateway"));
    assert!(!manifest.contains("llm"));
}
```

- [ ] **Step 8: 테스트 실행**

Run: `cargo test -p automation-core`
Expected: PASS (mock 3 + no_ai_gateway 1 = 4 tests).

- [ ] **Step 9: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): runtime types, adapter seams, mocks, ai-gateway guard"
```

---

## Task 3: `validate` + `interpret` (순수 rule 분석)

**Files:**
- Create: `crates/automation-core/src/validate.rs`
- Create: `crates/automation-core/src/interpret.rs`
- Modify: `crates/automation-core/src/lib.rs`
- Create: `crates/automation-core/tests/validate.rs`
- Create: `crates/automation-core/tests/interpret.rs`

**Interfaces:**
- Consumes: `automation_state::{InteractionRuleSet, TriggerSpec, ActionSpec, ActionTarget}`, `resource_resolution::ResourceBindingMap`, `desired_state::ResourceKey`, `crate::event::{RuntimeEvent, EventKind}`, `crate::plan::{ActionPlan, PlannedAction}`.
- Produces: `validate(&InteractionRuleSet, &ResourceBindingMap) -> Result<(), Vec<ValidationError>>`, `ValidationError`(enum), `interpret(&RuntimeEvent, &InteractionRuleSet, &ResourceBindingMap) -> Option<ActionPlan>`.
- 계약: `interpret`는 validate 통과를 전제로 한다. 매칭 rule 없음 → `None`. validate가 중복 trigger를 거부하므로 하나의 이벤트는 **최대 1개** rule에만 매칭되고 `.find()`가 그 첫 매칭을 반환한다. (해소 실패는 이론상 발생하지 않으나, 발생 시에도 panic 없이 `None`.)

- [ ] **Step 1: 실패 테스트 작성 — `crates/automation-core/tests/validate.rs`**

```rust
use std::collections::BTreeMap;

use automation_core::validate::{validate, ValidationError};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::RoleId;
use resource_resolution::ResourceBindingMap;

fn bindings_with(role: &str, id: u64) -> ResourceBindingMap {
    let mut map = ResourceBindingMap::default();
    map.role_bindings
        .insert(ResourceKey(role.to_string()), RoleId(id));
    map
}

fn rule(key: &str, component: &str, role: &str) -> InteractionRule {
    InteractionRule {
        key: key.to_string(),
        trigger: TriggerSpec::ButtonClick {
            component: component.to_string(),
        },
        actions: vec![ActionSpec::GrantRole {
            role: ResourceKey(role.to_string()),
            target: ActionTarget::Actor,
        }],
    }
}

fn panel(button: &str) -> PanelSpec {
    PanelSpec {
        key: "p".to_string(),
        channel: ResourceKey("c".to_string()),
        content: "x".to_string(),
        buttons: vec![ButtonSpec {
            key: button.to_string(),
            label: "b".to_string(),
        }],
    }
}

#[test]
fn valid_ruleset_passes() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        rules: vec![rule("r1", "verify_button", "verified")],
    };
    assert!(validate(&set, &bindings_with("verified", 100)).is_ok());
}

#[test]
fn missing_role_ref_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        rules: vec![rule("r1", "verify_button", "ghost_role")],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownRoleRef {
        rule: "r1".to_string(),
        role: ResourceKey("ghost_role".to_string()),
    }));
}

#[test]
fn unknown_button_ref_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        rules: vec![rule("r1", "ghost_button", "verified")],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownButtonRef {
        rule: "r1".to_string(),
        component: "ghost_button".to_string(),
    }));
}

#[test]
fn duplicate_rule_and_button_keys_fail() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "p".to_string(),
            channel: ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![
                ButtonSpec {
                    key: "b".to_string(),
                    label: "one".to_string(),
                },
                ButtonSpec {
                    key: "b".to_string(),
                    label: "two".to_string(),
                },
            ],
        }],
        rules: vec![rule("dup", "b", "verified"), rule("dup", "b", "verified")],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateButtonKey("b".to_string())));
    assert!(errors.contains(&ValidationError::DuplicateRuleKey("dup".to_string())));
}

#[test]
fn duplicate_button_trigger_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        rules: vec![
            rule("r1", "verify_button", "verified"),
            rule("r2", "verify_button", "verified"),
        ],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::ConflictingTrigger {
        component: "verify_button".to_string(),
    }));
}

#[test]
fn empty_respond_content_fails() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![panel("verify_button")],
        rules: vec![InteractionRule {
            key: "r1".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "verify_button".to_string(),
            },
            actions: vec![ActionSpec::RespondEphemeral {
                content: "   ".to_string(),
            }],
        }],
    };
    let errors = validate(&set, &bindings_with("verified", 100)).unwrap_err();
    assert!(errors.contains(&ValidationError::EmptyResponseContent {
        rule: "r1".to_string(),
    }));
}
```

- [ ] **Step 2: 실행해서 실패(컴파일 에러) 확인**

Run: `cargo test -p automation-core --test validate`
Expected: FAIL — `unresolved import automation_core::validate` / `ValidationError` 미정의.

- [ ] **Step 3: `crates/automation-core/src/validate.rs` 구현**

```rust
use std::collections::BTreeSet;

use automation_state::{ActionSpec, InteractionRuleSet, TriggerSpec};
use desired_state::ResourceKey;
use resource_resolution::ResourceBindingMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    DuplicatePanelKey(String),
    DuplicateButtonKey(String),
    DuplicateRuleKey(String),
    UnknownButtonRef { rule: String, component: String },
    UnknownRoleRef { rule: String, role: ResourceKey },
    ConflictingTrigger { component: String },
    EmptyResponseContent { rule: String },
}

pub fn validate(
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    let mut panel_keys: BTreeSet<&str> = BTreeSet::new();
    let mut button_keys: BTreeSet<String> = BTreeSet::new();
    for panel in &ruleset.panels {
        if !panel_keys.insert(panel.key.as_str()) {
            errors.push(ValidationError::DuplicatePanelKey(panel.key.clone()));
        }
        for button in &panel.buttons {
            if !button_keys.insert(button.key.clone()) {
                errors.push(ValidationError::DuplicateButtonKey(button.key.clone()));
            }
        }
    }

    let mut rule_keys: BTreeSet<&str> = BTreeSet::new();
    let mut trigger_components: BTreeSet<String> = BTreeSet::new();
    for rule in &ruleset.rules {
        if !rule_keys.insert(rule.key.as_str()) {
            errors.push(ValidationError::DuplicateRuleKey(rule.key.clone()));
        }
        match &rule.trigger {
            TriggerSpec::ButtonClick { component } => {
                if !button_keys.contains(component) {
                    errors.push(ValidationError::UnknownButtonRef {
                        rule: rule.key.clone(),
                        component: component.clone(),
                    });
                }
                if !trigger_components.insert(component.clone()) {
                    errors.push(ValidationError::ConflictingTrigger {
                        component: component.clone(),
                    });
                }
            }
        }
        for action in &rule.actions {
            match action {
                ActionSpec::GrantRole { role, .. } => {
                    if !bindings.role_bindings.contains_key(role) {
                        errors.push(ValidationError::UnknownRoleRef {
                            rule: rule.key.clone(),
                            role: role.clone(),
                        });
                    }
                }
                ActionSpec::RespondEphemeral { content } => {
                    if content.trim().is_empty() {
                        errors.push(ValidationError::EmptyResponseContent {
                            rule: rule.key.clone(),
                        });
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
```

- [ ] **Step 4: 실패 테스트 작성 — `crates/automation-core/tests/interpret.rs`**

```rust
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::interpret::interpret;
use automation_core::plan::PlannedAction;
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, RoleId, UserId};
use resource_resolution::ResourceBindingMap;

fn fixture() -> (InteractionRuleSet, ResourceBindingMap) {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "verify_panel".to_string(),
            channel: ResourceKey("verify_channel".to_string()),
            content: "click".to_string(),
            buttons: vec![ButtonSpec {
                key: "verify_button".to_string(),
                label: "Verify".to_string(),
            }],
        }],
        rules: vec![InteractionRule {
            key: "verify_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "verify_button".to_string(),
            },
            actions: vec![
                ActionSpec::GrantRole {
                    role: ResourceKey("verified_member".to_string()),
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: "welcome".to_string(),
                },
            ],
        }],
    };
    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey("verified_member".to_string()), RoleId(555));
    (set, bindings)
}

fn click(component: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(42),
        kind: EventKind::ButtonClick {
            component: component.to_string(),
        },
    }
}

#[test]
fn matching_button_click_finds_rule() {
    let (set, bindings) = fixture();
    assert!(interpret(&click("verify_button"), &set, &bindings).is_some());
}

#[test]
fn plan_grants_role_to_actor_and_responds() {
    let (set, bindings) = fixture();
    let plan = interpret(&click("verify_button"), &set, &bindings).unwrap();
    assert_eq!(
        plan.steps,
        vec![
            PlannedAction::GrantRole {
                role: RoleId(555),
                target: UserId(42),
            },
            PlannedAction::RespondEphemeral {
                content: "welcome".to_string(),
            },
        ]
    );
}

#[test]
fn unmatched_button_click_is_none() {
    let (set, bindings) = fixture();
    assert!(interpret(&click("other_button"), &set, &bindings).is_none());
}
```

- [ ] **Step 5: `crates/automation-core/src/interpret.rs` 구현**

```rust
use automation_state::{ActionSpec, ActionTarget, InteractionRuleSet, TriggerSpec};
use resource_resolution::ResourceBindingMap;

use crate::event::{EventKind, RuntimeEvent};
use crate::plan::{ActionPlan, PlannedAction};

pub fn interpret(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
) -> Option<ActionPlan> {
    let rule = ruleset
        .rules
        .iter()
        .find(|rule| trigger_matches(&rule.trigger, &event.kind))?;

    let mut steps = Vec::new();
    for action in &rule.actions {
        match action {
            ActionSpec::GrantRole { role, target } => {
                let role_id = *bindings.role_bindings.get(role)?;
                let target_id = match target {
                    ActionTarget::Actor => event.actor,
                };
                steps.push(PlannedAction::GrantRole {
                    role: role_id,
                    target: target_id,
                });
            }
            ActionSpec::RespondEphemeral { content } => {
                steps.push(PlannedAction::RespondEphemeral {
                    content: content.clone(),
                });
            }
        }
    }

    Some(ActionPlan { steps })
}

fn trigger_matches(trigger: &TriggerSpec, kind: &EventKind) -> bool {
    match (trigger, kind) {
        (
            TriggerSpec::ButtonClick { component },
            EventKind::ButtonClick {
                component: clicked,
            },
        ) => component == clicked,
    }
}
```

- [ ] **Step 6: `crates/automation-core/src/lib.rs`에 모듈 추가**

`pub mod adapter;` 블록과 재노출을 다음으로 교체:

```rust
pub mod adapter;
pub mod event;
pub mod interpret;
pub mod mock;
pub mod plan;
pub mod validate;

pub use adapter::{AdapterError, AdapterErrorKind, DiscordMutationAdapter, InteractionResponder};
pub use event::{EventKind, RuntimeContext, RuntimeEvent};
pub use interpret::interpret;
pub use mock::{MockInteractionResponder, MockMutationAdapter, MutationCall, ResponderCall};
pub use plan::{ActionPlan, PlannedAction};
pub use validate::{validate, ValidationError};
```

- [ ] **Step 7: 테스트 실행**

Run: `cargo test -p automation-core --test validate --test interpret`
Expected: PASS (validate 6 + interpret 3).

- [ ] **Step 8: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): rule validation and deterministic interpreter"
```

---

## Task 4: `run` + `handle_event` (seam으로 결정론적 실행)

**Files:**
- Create: `crates/automation-core/src/run.rs`
- Modify: `crates/automation-core/src/lib.rs`
- Create: `crates/automation-core/tests/run.rs`

**Interfaces:**
- Consumes: `crate::event::{RuntimeEvent, RuntimeContext}`, `crate::plan::{ActionPlan, PlannedAction}`, `crate::adapter::{AdapterError, DiscordMutationAdapter, InteractionResponder}`, `crate::interpret::interpret`, `automation_state::InteractionRuleSet`, `resource_resolution::ResourceBindingMap`.
- Produces: `run(&RuntimeContext, &ActionPlan, &impl DiscordMutationAdapter, &impl InteractionResponder) -> Result<(), AdapterError>` (fail-fast), `HandleOutcome { Executed, NoOp }`, `handle_event(&RuntimeEvent, &InteractionRuleSet, &ResourceBindingMap, &impl DiscordMutationAdapter, &impl InteractionResponder) -> Result<HandleOutcome, AdapterError>` (매칭 없으면 `Ok(NoOp)`, 실행하면 `Ok(Executed)`).

- [ ] **Step 1: 실패 테스트 작성 — `crates/automation-core/tests/run.rs`**

```rust
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall, ResponderCall};
use automation_core::run::{handle_event, HandleOutcome};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn fixture() -> (InteractionRuleSet, ResourceBindingMap) {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "verify_panel".to_string(),
            channel: ResourceKey("verify_channel".to_string()),
            content: "click".to_string(),
            buttons: vec![ButtonSpec {
                key: "verify_button".to_string(),
                label: "Verify".to_string(),
            }],
        }],
        rules: vec![InteractionRule {
            key: "verify_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "verify_button".to_string(),
            },
            actions: vec![
                ActionSpec::GrantRole {
                    role: ResourceKey("verified_member".to_string()),
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: "welcome".to_string(),
                },
            ],
        }],
    };
    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey("verified_member".to_string()), RoleId(555));
    (set, bindings)
}

fn click(component: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(9),
        actor: UserId(42),
        kind: EventKind::ButtonClick {
            component: component.to_string(),
        },
    }
}

#[test]
fn matching_event_grants_role_and_responds() {
    let (set, bindings) = fixture();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();

    let outcome = block_on(handle_event(
        &click("verify_button"),
        &set,
        &bindings,
        &mutation,
        &responder,
    ))
    .unwrap();

    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::GrantRole {
            guild: GuildId(9),
            member: UserId(42),
            role: RoleId(555),
        }]
    );
    assert_eq!(
        responder.calls(),
        vec![ResponderCall::RespondEphemeral {
            content: "welcome".to_string(),
        }]
    );
}

#[test]
fn unmatched_event_is_noop_with_no_calls() {
    let (set, bindings) = fixture();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();

    let outcome = block_on(handle_event(
        &click("other_button"),
        &set,
        &bindings,
        &mutation,
        &responder,
    ))
    .unwrap();

    assert_eq!(outcome, HandleOutcome::NoOp);
    assert!(mutation.calls().is_empty());
    assert!(responder.calls().is_empty());
}

#[test]
fn mutation_failure_propagates() {
    use automation_core::adapter::{AdapterError, AdapterErrorKind};

    let (set, bindings) = fixture();
    let mutation = MockMutationAdapter::failing(AdapterError::new(
        AdapterErrorKind::Forbidden,
        "missing perms",
    ));
    let responder = MockInteractionResponder::new();

    let result = block_on(handle_event(
        &click("verify_button"),
        &set,
        &bindings,
        &mutation,
        &responder,
    ));

    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
    assert!(responder.calls().is_empty());
}
```

- [ ] **Step 2: 실행해서 실패 확인**

Run: `cargo test -p automation-core --test run`
Expected: FAIL — `unresolved import automation_core::run`.

- [ ] **Step 3: `crates/automation-core/src/run.rs` 구현**

```rust
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;

use crate::adapter::{AdapterError, DiscordMutationAdapter, InteractionResponder};
use crate::event::{RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{ActionPlan, PlannedAction};

pub async fn run(
    context: &RuntimeContext,
    plan: &ActionPlan,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
) -> Result<(), AdapterError> {
    for step in &plan.steps {
        match step {
            PlannedAction::GrantRole { role, target } => {
                mutation.grant_role(context.guild_id, *target, *role).await?;
            }
            PlannedAction::RespondEphemeral { content } => {
                responder.respond_ephemeral(content.clone()).await?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleOutcome {
    Executed,
    NoOp,
}

pub async fn handle_event(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
) -> Result<HandleOutcome, AdapterError> {
    match interpret(event, ruleset, bindings) {
        Some(plan) => {
            let context = RuntimeContext::from_event(event);
            run(&context, &plan, mutation, responder).await?;
            Ok(HandleOutcome::Executed)
        }
        None => Ok(HandleOutcome::NoOp),
    }
}
```

- [ ] **Step 4: `crates/automation-core/src/lib.rs`에 run 추가**

`pub mod plan;` 아래에 `pub mod run;` 추가, 재노출에 다음 추가:

```rust
pub use run::{handle_event, run, HandleOutcome};
```

- [ ] **Step 5: 테스트 실행**

Run: `cargo test -p automation-core --test run`
Expected: PASS (3 tests).

- [ ] **Step 6: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): deterministic run and handle_event over seams"
```

---

## Task 5: `policy` (동작 안전 정적 분석)

**Files:**
- Create: `crates/automation-core/src/policy.rs`
- Modify: `crates/automation-core/src/lib.rs`
- Create: `crates/automation-core/tests/policy.rs`

**Interfaces:**
- Consumes: `automation_state::{InteractionRuleSet, ActionSpec}`, `desired_state::ResourceKey`, `discord_model::Permissions`, `std::collections::BTreeMap`.
- Produces: `PolicyFinding { rule: String, role: ResourceKey, reason: String }`, `privileged_mask() -> Permissions`, `analyze(&InteractionRuleSet, &BTreeMap<ResourceKey, Permissions>) -> Vec<PolicyFinding>` (빈 벡터 = 허용).

- [ ] **Step 1: 실패 테스트 작성 — `crates/automation-core/tests/policy.rs`**

```rust
use std::collections::BTreeMap;

use automation_core::policy::analyze;
use automation_state::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::Permissions;

fn rule(key: &str, role: &str) -> InteractionRule {
    InteractionRule {
        key: key.to_string(),
        trigger: TriggerSpec::ButtonClick {
            component: "b".to_string(),
        },
        actions: vec![ActionSpec::GrantRole {
            role: ResourceKey(role.to_string()),
            target: ActionTarget::Actor,
        }],
    }
}

fn roles() -> BTreeMap<ResourceKey, Permissions> {
    let mut roles = BTreeMap::new();
    roles.insert(
        ResourceKey("admin".to_string()),
        Permissions::ADMINISTRATOR,
    );
    roles.insert(
        ResourceKey("verified".to_string()),
        Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
    );
    roles
}

#[test]
fn granting_privileged_role_is_flagged() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        rules: vec![rule("r1", "admin")],
    };
    let findings = analyze(&set, &roles());
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "r1");
    assert_eq!(findings[0].role, ResourceKey("admin".to_string()));
}

#[test]
fn granting_ordinary_role_is_allowed() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        rules: vec![rule("r1", "verified")],
    };
    assert!(analyze(&set, &roles()).is_empty());
}
```

- [ ] **Step 2: 실행해서 실패 확인**

Run: `cargo test -p automation-core --test policy`
Expected: FAIL — `unresolved import automation_core::policy`.

- [ ] **Step 3: `crates/automation-core/src/policy.rs` 구현**

```rust
use std::collections::BTreeMap;

use automation_state::{ActionSpec, InteractionRuleSet};
use desired_state::ResourceKey;
use discord_model::Permissions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyFinding {
    pub rule: String,
    pub role: ResourceKey,
    pub reason: String,
}

pub fn privileged_mask() -> Permissions {
    Permissions::ADMINISTRATOR
        | Permissions::MANAGE_GUILD
        | Permissions::MANAGE_ROLES
        | Permissions::MANAGE_CHANNELS
        | Permissions::BAN_MEMBERS
        | Permissions::KICK_MEMBERS
        | Permissions::MODERATE_MEMBERS
}

pub fn analyze(
    ruleset: &InteractionRuleSet,
    roles: &BTreeMap<ResourceKey, Permissions>,
) -> Vec<PolicyFinding> {
    let mask = privileged_mask();
    let mut findings = Vec::new();
    for rule in &ruleset.rules {
        for action in &rule.actions {
            if let ActionSpec::GrantRole { role, .. } = action {
                if let Some(permissions) = roles.get(role) {
                    if permissions.intersects(mask) {
                        findings.push(PolicyFinding {
                            rule: rule.key.clone(),
                            role: role.clone(),
                            reason: "grants a privileged role".to_string(),
                        });
                    }
                }
            }
        }
    }
    findings
}
```

- [ ] **Step 4: `crates/automation-core/src/lib.rs`에 policy 추가**

`pub mod plan;` 아래에 `pub mod policy;` 추가, 재노출에 다음 추가:

```rust
pub use policy::{analyze, privileged_mask, PolicyFinding};
```

- [ ] **Step 5: 테스트 실행**

Run: `cargo test -p automation-core --test policy`
Expected: PASS (2 tests).

- [ ] **Step 6: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): privileged-role grant policy analysis"
```

---

## Task 6: 워크스페이스 검증 게이트

**Files:** 없음 (신규 코드 없음, 게이트만).

- [ ] **Step 1: 전체 빌드**

Run: `cargo build`
Expected: 성공, 경고 0.

- [ ] **Step 2: 전체 테스트**

Run: `cargo test`
Expected: 전부 PASS. 신규 = automation-state 7 (스키마 6 + no_ai_gateway 1) + automation-core 18 (mock 3 + no_ai_gateway 1 + validate 6 + interpret 3 + run 3 + policy 2) = 25. 기존 159개 무변경 → 총 184.

- [ ] **Step 3: clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 경고/에러 0.

- [ ] **Step 4: fmt 확인**

Run: `cargo fmt --all -- --check`
Expected: diff 없음.

- [ ] **Step 5: 푸시**

```bash
git push origin main
```

---

## Self-Review (스펙 대비)

- **스펙 §6 타입 모델 커버리지:** InteractionRuleSet/InteractionRule/TriggerSpec/ActionSpec/ActionTarget/PanelSpec/ButtonSpec(Task 1), RuntimeEvent/EventKind/RuntimeContext/ActionPlan/PlannedAction(Task 2) ✅. `ConditionSpec`는 의도적 제외(사용자 승인 — §Scope 결정 1).
- **스펙 §7 validate:** key 유일성 + button ref + role ref + **중복 trigger 거부** + **빈 content 거부**(Task 3, validate.rs) ✅.
- **스펙 §8 policy:** privileged GrantRole 플래그(ADMINISTRATOR/MANAGE_GUILD/MANAGE_ROLES/MANAGE_CHANNELS/BAN/KICK/MODERATE)(Task 5) ✅.
- **스펙 §9 interpreter:** trigger match → ActionPlan, actor/role 해소, no-match None, LLM 없음(Task 3, interpret.rs) ✅.
- **스펙 §10 seam 분리:** DiscordMutationAdapter / InteractionResponder + Mock(Task 2) ✅.
- **스펙 §11 테스트 7 + policy:** (1)matching→Task3 interpret, (2)plan grant+respond→Task3, (3)no-match no-op→Task3+Task4, (4)missing role ref→Task3 validate, (5)dup key→Task3 validate, (6)RespondEphemeral 실행→Task4 run, (7)no ai-gateway→Task1·Task2, +policy privileged→Task5 ✅.
- **사용자 보강 4개:** (1)ConditionSpec 제외→Task1 스키마 미포함, (2)unknown field 거부→6타입 `deny_unknown_fields` + Task1 거부 테스트 3개(conditions/unknown variant/variant 내부 필드), (3)중복 button trigger→validate `ConflictingTrigger` + 테스트, (4)ai-gateway 미의존→**두 크레이트** 각각 no_ai_gateway 가드 ✅.
- **스펙 §12 forbidden:** live gateway/endpoint/DB/modal/dynamic template/condition-lang/retry/event-time-AI 전부 미구현 ✅.
- **Placeholder 스캔:** 코드 블록 전부 완성형, TBD/TODO 없음 ✅.
- **타입 일관성:** `validate`/`interpret`/`run`/`handle_event`/`analyze` 시그니처가 Task 간 consumes/produces와 일치. `HandleOutcome`, `ResourceBindingMap.role_bindings`(BTreeMap), `Permissions::intersects`, `deny_unknown_fields`+internal-tag(serde 1.0.228 실증), `block_on` 사용 실제 API 확인 ✅.
- **주석:** 코드 블록에 `//`/`///`/`//!` 없음 ✅.

---

## Codex 핸드오프 (권장 청크 — 2~3개 묶음)

- **청크 A** = Task 1 + Task 2 (두 크레이트 스캐폴드 + 스키마 + 런타임 타입 + seam + mock + 의존성 가드). 커밋 2개.
- **청크 B** = Task 3 + Task 4 (validate + interpret + run + handle_event). 커밋 2개.
- **청크 C** = Task 5 + Task 6 (policy + 전체 검증 게이트 + push). 커밋 1개 + push.

각 청크 끝에서 `cargo test`/`clippy --all-targets -- -D warnings`/`fmt --all -- --check` 통과 확인. 최종 청크 뒤 `git push origin main`. **live/토큰/DB/modal/template/condition 평가 없음.** 완료 보고 시 크레이트별 테스트 수(automation-state 7 / automation-core 18 / 기존 159 무변경 → 총 184)를 명시.
