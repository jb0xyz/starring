# Phase 16c — Modal / OpenModal Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 모달 왕복(버튼→모달→제출→정적 액션)을 순수 코어로 구현. 입력값은 RuntimeEvent에 캡처·보존만, interpolation 없음.

**Architecture:** automation-state에 ModalSpec/OpenModal/ModalSubmit 추가, automation-core에 EventKind::ModalSubmit/ModalPresentation/PlannedAction::OpenModal/`open_modal`(default unsupported)/validate/interpret/run 확장. **automation-runtime은 무수정**(default open_modal 상속으로 16b 컴파일 유지). Mock 결정론 테스트.

**Tech Stack:** Rust(edition 2021), serde(deny_unknown_fields), `futures::executor::block_on`. live/DB/template 없음.

## Global Constraints

- **코드 주석 절대 금지**(`//`,`///`,`//!`). **Codex 구현**(코드 그대로).
- **automation-runtime 절대 무수정** — trait의 `open_modal` default(unsupported)로 16b TwilightInteractionResponder가 그대로 컴파일돼야 함(실증: default async fn 본문 stable 컴파일 확인).
- **입력값 interpolation 금지** — ModalSubmit inputs는 RuntimeEvent에만 캡처, static plan으로 흘리지 않음(16e에서 소비).
- 6타입+신규 3타입 전부 `#[serde(deny_unknown_fields)]`. ID string-serde. `#[allow(async_fn_in_trait)]` 유지.
- **완료 게이트**: build/test/clippy(`--all-targets -- -D warnings`)/fmt(`--all -- --check`). 완료 후 `git push origin main`.
- **live/토큰/DB/template/dynamic/CreateChannel/CreateRole 없음.**

---

## File Structure

**automation-state:**
- Create: `crates/automation-state/src/modal.rs` — ModalSpec/ModalFieldSpec/ModalFieldStyle
- Modify: `crates/automation-state/src/rule.rs` — modals 필드 + ModalSubmit trigger + OpenModal action + 기존 테스트 1개 교체
- Modify: `crates/automation-state/src/lib.rs` — modal 모듈/재노출

**automation-core:**
- Modify: `crates/automation-core/src/event.rs` — EventKind::ModalSubmit
- Modify: `crates/automation-core/src/plan.rs` — ModalPresentation + PlannedAction::OpenModal
- Modify: `crates/automation-core/src/adapter.rs` — AdapterErrorKind::Unsupported + open_modal default
- Modify: `crates/automation-core/src/mock.rs` — ResponderCall::OpenModal + open_modal override + 테스트
- Modify: `crates/automation-core/src/validate.rs` — modal 검증 4종
- Modify: `crates/automation-core/src/interpret.rs` — 두 이벤트 kind + OpenModal action
- Modify: `crates/automation-core/src/run.rs` — OpenModal 실행
- Modify: `crates/automation-core/src/lib.rs` — ModalPresentation 재노출
- Create: `crates/automation-core/tests/modal.rs` — 행위 테스트

---

## Task 1: automation-state — modal 스키마

**Files:** modal.rs(new), rule.rs(modify), lib.rs(modify).

**Interfaces:**
- Produces: `ModalSpec { key, title, fields: Vec<ModalFieldSpec> }`, `ModalFieldSpec { key, label, style: ModalFieldStyle, required: bool }`, `ModalFieldStyle::{Short, Paragraph}`, `TriggerSpec::ModalSubmit { modal: String }`, `ActionSpec::OpenModal { modal: String }`, `InteractionRuleSet.modals: Vec<ModalSpec>`.

- [ ] **Step 1: `crates/automation-state/src/modal.rs` 작성**

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalSpec {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub fields: Vec<ModalFieldSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModalFieldSpec {
    pub key: String,
    pub label: String,
    pub style: ModalFieldStyle,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModalFieldStyle {
    Short,
    Paragraph,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModalSpec {
        ModalSpec {
            key: "study_room_modal".to_string(),
            title: "Create study room".to_string(),
            fields: vec![ModalFieldSpec {
                key: "room_name".to_string(),
                label: "Room name".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
            }],
        }
    }

    #[test]
    fn modal_spec_roundtrips() {
        let modal = sample();
        let json = serde_json::to_string(&modal).unwrap();
        let back: ModalSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(modal, back);
        assert!(json.contains(r#""style":"short""#));
    }

    #[test]
    fn unknown_field_in_modal_is_rejected() {
        let json = r#"{"key":"m","title":"t","fields":[],"evil":1}"#;
        assert!(serde_json::from_str::<ModalSpec>(json).is_err());
    }
}
```

- [ ] **Step 2: `rule.rs` — modal import + modals 필드**

`use crate::panel::PanelSpec;` 다음 줄에 추가:

```rust
use crate::modal::ModalSpec;
```

`InteractionRuleSet`의 `panels` 필드와 `rules` 필드 사이에 modals 추가 — 즉 struct 본문을 다음으로 교체:

```rust
pub struct InteractionRuleSet {
    pub version: u32,
    #[serde(default)]
    pub panels: Vec<PanelSpec>,
    #[serde(default)]
    pub modals: Vec<ModalSpec>,
    #[serde(default)]
    pub rules: Vec<InteractionRule>,
}
```

- [ ] **Step 3: `rule.rs` — TriggerSpec/ActionSpec 확장**

`TriggerSpec` enum 본문을 교체:

```rust
pub enum TriggerSpec {
    ButtonClick { component: String },
    ModalSubmit { modal: String },
}
```

`ActionSpec` enum 본문을 교체:

```rust
pub enum ActionSpec {
    GrantRole {
        role: ResourceKey,
        target: ActionTarget,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal {
        modal: String,
    },
}
```

- [ ] **Step 4: `rule.rs` — 기존 테스트 교체 (open_modal이 이제 유효하므로)**

`unknown_action_type_is_rejected` 테스트를 다음으로 교체(미지원 액션 타입을 여전히 거부하는지 확인):

```rust
    #[test]
    fn unknown_action_type_is_rejected() {
        let json = r#"{"type":"create_channel","channel":"x"}"#;
        assert!(serde_json::from_str::<ActionSpec>(json).is_err());
    }
```

- [ ] **Step 5: `crates/automation-state/src/lib.rs` 교체**

```rust
pub mod modal;
pub mod panel;
pub mod rule;

pub use modal::{ModalFieldSpec, ModalFieldStyle, ModalSpec};
pub use panel::{ButtonSpec, PanelSpec};
pub use rule::{ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, TriggerSpec};
```

- [ ] **Step 6: 테스트**

Run: `cargo test -p automation-state`
Expected: PASS (기존 6 + modal 2 + no_ai_gateway 1 = 9). 교체된 unknown_action_type도 통과.

- [ ] **Step 7: 커밋**

```bash
git add crates/automation-state
git commit -m "feat(automation-state): modal schema (ModalSpec, ModalSubmit, OpenModal)"
```

---

## Task 2: automation-core — 타입 · seam (event · plan · adapter · mock)

**Files:** event.rs, plan.rs, adapter.rs, mock.rs, lib.rs (modify).

**Interfaces:**
- Produces: `EventKind::ModalSubmit { modal, inputs: BTreeMap<String,String> }`, `ModalPresentation { key, title, fields: Vec<ModalFieldSpec> }`, `PlannedAction::OpenModal(ModalPresentation)`, `AdapterErrorKind::Unsupported`, `InteractionResponder::open_modal(&self, &ModalPresentation)`(default unsupported), `ResponderCall::OpenModal { modal }`, Mock open_modal.

- [ ] **Step 1: `crates/automation-core/src/event.rs` 교체**

```rust
use std::collections::BTreeMap;

use discord_model::{GuildId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    ButtonClick {
        component: String,
    },
    ModalSubmit {
        modal: String,
        inputs: BTreeMap<String, String>,
    },
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

- [ ] **Step 2: `crates/automation-core/src/plan.rs` 교체**

```rust
use automation_state::ModalFieldSpec;
use discord_model::{RoleId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPlan {
    pub steps: Vec<PlannedAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModalPresentation {
    pub key: String,
    pub title: String,
    pub fields: Vec<ModalFieldSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedAction {
    GrantRole { role: RoleId, target: UserId },
    RespondEphemeral { content: String },
    OpenModal(ModalPresentation),
}
```

- [ ] **Step 3: `crates/automation-core/src/adapter.rs` 교체** (Unsupported + open_modal default)

```rust
use serde::{Deserialize, Serialize};

use discord_model::{GuildId, RoleId, UserId};

use crate::plan::ModalPresentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKind {
    Forbidden,
    NotFound,
    RateLimited,
    Network,
    Unsupported,
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

    async fn open_modal(&self, _modal: &ModalPresentation) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "open_modal is not supported",
        ))
    }
}
```

- [ ] **Step 4: `mock.rs` — import + ResponderCall::OpenModal + open_modal override**

`use crate::adapter::{...};` 다음 줄에 추가:

```rust
use crate::plan::ModalPresentation;
```

`ResponderCall` enum 본문을 교체:

```rust
pub enum ResponderCall {
    RespondEphemeral { content: String },
    OpenModal { modal: String },
}
```

`impl InteractionResponder for MockInteractionResponder`의 닫는 `}` 앞(respond_ephemeral 다음)에 open_modal 추가:

```rust
    async fn open_modal(&self, modal: &ModalPresentation) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(ResponderCall::OpenModal {
                modal: modal.key.clone(),
            });
        Ok(())
    }
```

- [ ] **Step 5: `mock.rs` — in-module 테스트 추가**

`#[cfg(test)] mod tests`의 `responder_records` 테스트 다음에 추가:

```rust
    #[test]
    fn responder_records_open_modal() {
        use crate::plan::ModalPresentation;
        let mock = MockInteractionResponder::new();
        let presentation = ModalPresentation {
            key: "study_room_modal".to_string(),
            title: "Create study room".to_string(),
            fields: vec![],
        };
        block_on(mock.open_modal(&presentation)).unwrap();
        assert_eq!(
            mock.calls(),
            vec![ResponderCall::OpenModal {
                modal: "study_room_modal".to_string(),
            }]
        );
    }
```

- [ ] **Step 6: `crates/automation-core/src/lib.rs` — plan 재노출 교체**

`pub use plan::{ActionPlan, PlannedAction};` 를 다음으로 교체:

```rust
pub use plan::{ActionPlan, ModalPresentation, PlannedAction};
```

- [ ] **Step 7: 빌드 + 테스트**

Run: `cargo build -p automation-core` then `cargo test -p automation-core`
Expected: 빌드 성공(경고 0), mock 4(기존 3 + open_modal 1) 등 통과. **automation-runtime도 빌드되는지 확인:** `cargo build -p automation-runtime`(default open_modal 상속) 성공.

- [ ] **Step 8: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): modal types, ModalSubmit event, open_modal seam (default unsupported)"
```

---

## Task 3: automation-core — validate · interpret · run + 행위 테스트

**Files:** validate.rs, interpret.rs, run.rs (modify), tests/modal.rs (new).

**Interfaces:**
- Produces: validate(modal key/field 유일성, OpenModal/ModalSubmit ref, modal trigger 충돌), interpret(ButtonClick+ModalSubmit, OpenModal action 해소), run(OpenModal 실행).

- [ ] **Step 1: `crates/automation-core/src/validate.rs` 교체**

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
    DuplicateModalKey(String),
    DuplicateModalFieldKey { modal: String, field: String },
    UnknownModalRef { rule: String, modal: String },
    ConflictingModalTrigger { modal: String },
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

    let mut modal_keys: BTreeSet<String> = BTreeSet::new();
    for modal in &ruleset.modals {
        if !modal_keys.insert(modal.key.clone()) {
            errors.push(ValidationError::DuplicateModalKey(modal.key.clone()));
        }
        let mut field_keys: BTreeSet<&str> = BTreeSet::new();
        for field in &modal.fields {
            if !field_keys.insert(field.key.as_str()) {
                errors.push(ValidationError::DuplicateModalFieldKey {
                    modal: modal.key.clone(),
                    field: field.key.clone(),
                });
            }
        }
    }

    let mut rule_keys: BTreeSet<&str> = BTreeSet::new();
    let mut trigger_components: BTreeSet<String> = BTreeSet::new();
    let mut modal_triggers: BTreeSet<String> = BTreeSet::new();
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
            TriggerSpec::ModalSubmit { modal } => {
                if !modal_keys.contains(modal) {
                    errors.push(ValidationError::UnknownModalRef {
                        rule: rule.key.clone(),
                        modal: modal.clone(),
                    });
                }
                if !modal_triggers.insert(modal.clone()) {
                    errors.push(ValidationError::ConflictingModalTrigger {
                        modal: modal.clone(),
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
                ActionSpec::OpenModal { modal } => {
                    if !modal_keys.contains(modal) {
                        errors.push(ValidationError::UnknownModalRef {
                            rule: rule.key.clone(),
                            modal: modal.clone(),
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

- [ ] **Step 2: `crates/automation-core/src/interpret.rs` 교체**

```rust
use automation_state::{ActionSpec, ActionTarget, InteractionRuleSet, TriggerSpec};
use resource_resolution::ResourceBindingMap;

use crate::event::{EventKind, RuntimeEvent};
use crate::plan::{ActionPlan, ModalPresentation, PlannedAction};

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
            ActionSpec::OpenModal { modal } => {
                let spec = ruleset.modals.iter().find(|candidate| candidate.key == *modal)?;
                steps.push(PlannedAction::OpenModal(ModalPresentation {
                    key: spec.key.clone(),
                    title: spec.title.clone(),
                    fields: spec.fields.clone(),
                }));
            }
        }
    }

    Some(ActionPlan { steps })
}

fn trigger_matches(trigger: &TriggerSpec, kind: &EventKind) -> bool {
    match (trigger, kind) {
        (TriggerSpec::ButtonClick { component }, EventKind::ButtonClick { component: clicked }) => {
            component == clicked
        }
        (TriggerSpec::ModalSubmit { modal }, EventKind::ModalSubmit { modal: submitted, .. }) => {
            modal == submitted
        }
        _ => false,
    }
}
```

- [ ] **Step 3: `run.rs` — OpenModal arm 추가**

`run`의 match에서 `PlannedAction::RespondEphemeral { content } => { ... }` arm 다음에 추가:

```rust
            PlannedAction::OpenModal(modal) => {
                responder.open_modal(modal).await?;
            }
```

- [ ] **Step 4: `crates/automation-core/tests/modal.rs` 작성**

```rust
use std::collections::BTreeMap;

use automation_core::adapter::{AdapterError, AdapterErrorKind, InteractionResponder};
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::interpret::interpret;
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, ResponderCall};
use automation_core::plan::PlannedAction;
use automation_core::run::{handle_event, HandleOutcome};
use automation_core::validate::{validate, ValidationError};
use automation_state::{
    ActionSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle, ModalSpec,
    TriggerSpec,
};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn modal() -> ModalSpec {
    ModalSpec {
        key: "study_room_modal".to_string(),
        title: "Create study room".to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: "Room name".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
        }],
    }
}

fn ruleset() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![
            InteractionRule {
                key: "open_study_modal".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "create_study_button".to_string(),
                },
                actions: vec![ActionSpec::OpenModal {
                    modal: "study_room_modal".to_string(),
                }],
            },
            InteractionRule {
                key: "submit_study_modal".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: "study_room_modal".to_string(),
                },
                actions: vec![ActionSpec::RespondEphemeral {
                    content: "요청이 접수되었습니다.".to_string(),
                }],
            },
        ],
    }
}

fn button_event(component: &str) -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(42),
        kind: EventKind::ButtonClick {
            component: component.to_string(),
        },
    }
}

fn modal_event(modal_key: &str, room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(42),
        kind: EventKind::ModalSubmit {
            modal: modal_key.to_string(),
            inputs,
        },
    }
}

#[test]
fn valid_modal_ruleset_passes() {
    assert!(validate(&ruleset(), &ResourceBindingMap::default()).is_ok());
}

#[test]
fn duplicate_modal_key_fails() {
    let mut set = ruleset();
    set.modals.push(modal());
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateModalKey("study_room_modal".to_string())));
}

#[test]
fn duplicate_modal_field_key_fails() {
    let mut set = ruleset();
    set.modals[0].fields.push(ModalFieldSpec {
        key: "room_name".to_string(),
        label: "again".to_string(),
        style: ModalFieldStyle::Short,
        required: false,
    });
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateModalFieldKey {
        modal: "study_room_modal".to_string(),
        field: "room_name".to_string(),
    }));
}

#[test]
fn open_modal_unknown_ref_fails() {
    let mut set = ruleset();
    set.rules[0].actions = vec![ActionSpec::OpenModal {
        modal: "ghost_modal".to_string(),
    }];
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownModalRef {
        rule: "open_study_modal".to_string(),
        modal: "ghost_modal".to_string(),
    }));
}

#[test]
fn modal_submit_unknown_ref_fails() {
    let mut set = ruleset();
    set.rules[1].trigger = TriggerSpec::ModalSubmit {
        modal: "ghost_modal".to_string(),
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownModalRef {
        rule: "submit_study_modal".to_string(),
        modal: "ghost_modal".to_string(),
    }));
}

#[test]
fn duplicate_modal_trigger_fails() {
    let mut set = ruleset();
    set.rules.push(InteractionRule {
        key: "submit_again".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "study_room_modal".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: "dup".to_string(),
        }],
    });
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::ConflictingModalTrigger {
        modal: "study_room_modal".to_string(),
    }));
}

#[test]
fn button_click_produces_open_modal_plan() {
    let plan = interpret(
        &button_event("create_study_button"),
        &ruleset(),
        &ResourceBindingMap::default(),
    )
    .unwrap();
    match &plan.steps[..] {
        [PlannedAction::OpenModal(presentation)] => {
            assert_eq!(presentation.key, "study_room_modal");
            assert_eq!(presentation.title, "Create study room");
            assert_eq!(presentation.fields.len(), 1);
            assert_eq!(presentation.fields[0].key, "room_name");
        }
        other => panic!("expected single OpenModal, got {other:?}"),
    }
}

#[test]
fn modal_submit_event_captures_inputs() {
    let event = modal_event("study_room_modal", "cozy corner");
    match &event.kind {
        EventKind::ModalSubmit { modal, inputs } => {
            assert_eq!(modal, "study_room_modal");
            assert_eq!(inputs.get("room_name"), Some(&"cozy corner".to_string()));
        }
        other => panic!("expected ModalSubmit, got {other:?}"),
    }
    assert!(interpret(&event, &ruleset(), &ResourceBindingMap::default()).is_some());
}

#[test]
fn modal_submit_produces_static_plan() {
    let plan = interpret(
        &modal_event("study_room_modal", "cozy corner"),
        &ruleset(),
        &ResourceBindingMap::default(),
    )
    .unwrap();
    assert_eq!(
        plan.steps,
        vec![PlannedAction::RespondEphemeral {
            content: "요청이 접수되었습니다.".to_string(),
        }]
    );
}

#[test]
fn unknown_modal_submit_is_none() {
    assert!(interpret(
        &modal_event("ghost_modal", "x"),
        &ruleset(),
        &ResourceBindingMap::default()
    )
    .is_none());
}

#[test]
fn default_responder_open_modal_is_unsupported() {
    struct DefaultResponder;
    impl InteractionResponder for DefaultResponder {
        async fn respond_ephemeral(&self, _content: String) -> Result<(), AdapterError> {
            Ok(())
        }
    }

    let mutation = MockMutationAdapter::new();
    let responder = DefaultResponder;
    let result = block_on(handle_event(
        &button_event("create_study_button"),
        &ruleset(),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
    ));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Unsupported);
}

#[test]
fn mock_responder_runs_open_modal() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let outcome = block_on(handle_event(
        &button_event("create_study_button"),
        &ruleset(),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        responder.calls(),
        vec![ResponderCall::OpenModal {
            modal: "study_room_modal".to_string(),
        }]
    );
}
```

- [ ] **Step 5: 테스트**

Run: `cargo test -p automation-core`
Expected: PASS. tests/modal.rs 12개 포함.

- [ ] **Step 6: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): modal validate, two-event interpret, open_modal run"
```

---

## Task 4: 워크스페이스 검증 게이트 + push

- [ ] **Step 1: 빌드** — `cargo build` (경고 0, automation-runtime 포함 전부).
- [ ] **Step 2: 테스트** — `cargo test`. 신규 = automation-state 2 + automation-core 13(mock 1 + tests/modal 12) = 15. 기존 189 → 총 204. (교체 테스트는 개수 불변.)
- [ ] **Step 3: clippy** — `cargo clippy --all-targets -- -D warnings` (0).
- [ ] **Step 4: fmt** — `cargo fmt --all -- --check` (diff 없음).
- [ ] **Step 5: push** — `git push origin main`.

---

## Self-Review (스펙 대비)

- **스펙 §2 타입:** ModalSpec/ModalFieldSpec/ModalFieldStyle(Task1), EventKind::ModalSubmit/ModalPresentation/PlannedAction::OpenModal/AdapterErrorKind::Unsupported/open_modal default(Task2) ✅.
- **스펙 §3 validate:** modal key/field 유일성, OpenModal ref, ModalSubmit ref, (+ConflictingModalTrigger, 16a ButtonClick과 대칭 안전) ✅.
- **스펙 §4 interpret:** ButtonClick→OpenModal(modal 해소), ModalSubmit→static, inputs 미소비, no-match None ✅.
- **스펙 §5 run/Mock:** OpenModal→open_modal, Mock 기록, default unsupported 실패 ✅.
- **스펙 D1 캡처만:** ModalSubmit inputs는 event에만, static plan으로 안 흐름(test modal_submit_produces_static_plan은 content 정적) ✅.
- **스펙 D2/D3 무수정:** open_modal default → automation-runtime 무변경 컴파일(Task2 Step7에서 확인), 16b 안 깨짐 ✅. (default async fn 본문 stable 컴파일 실증됨.)
- **테스트 12+1:** 스펙 12 + ConflictingModalTrigger 1 → automation-state 2, automation-core 13. 기존 `unknown_action_type` 교체(open_modal 유효화) ✅.
- **타입 일관성:** ModalPresentation.key(=modal key; 스펙의 `modal` 필드를 명확히 `key`로), `.find(|c| c.key == *modal)`, trigger_matches `_ => false`, deny_unknown_fields, `ModalFieldSpec` 파생(Clone/Eq) ✅.
- **주석 없음** ✅.

**스펙 대비 사소한 조정(정당):** (1) ModalPresentation 필드명 `modal`→`key`(자기참조 `modal.modal` 회피, 의미 동일). (2) ConflictingModalTrigger 추가(16a 원칙 일관, 사용자 12테스트 밖 +1). — 사용자 검토 요망.

---

## Codex 핸드오프 (권장 2청크)

- **청크 A** = Task 1 + Task 2 (automation-state 스키마 + automation-core 타입/seam). **Task2 Step7에서 `cargo build -p automation-runtime` 성공 필수**(default open_modal 상속 확인). 커밋 2개.
- **청크 B** = Task 3 + Task 4 (validate/interpret/run + 행위 테스트 + 게이트 + push). 커밋 2개 + push.

**automation-runtime 절대 무수정.** 완료 보고: automation-state/core 테스트 수 + 전체(기대 204) + clippy/fmt + push 해시 + 이탈.
