# Phase 16e — Template + Sanitize Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** ModalSubmit input 값을 `${input.<key>}`로 RespondEphemeral 응답에 안전하게 사용. always-template + context-aware sanitize. 순수 코어.

**Architecture:** automation-core에 `template.rs`(parse/render/sanitize) 신설 + RuntimeContext에 inputs + validate 사전검사 + run 렌더. **RespondEphemeral 스키마·interpret·PlannedAction·automation-runtime·automation-state 무변경.**

**Tech Stack:** Rust(edition 2021). 순수 테스트(block_on 불필요한 부분은 순수 fn). live/DB 없음.

## Global Constraints

- **코드 주석 절대 금지.** **Codex 구현.**
- **automation-runtime / automation-state 무수정.** RespondEphemeral.content는 `String` 유지(always-template).
- **placeholder는 `${input.<field_key>}` 만.** missing input/미지원 변수/문법오류 = 에러(빈 문자열 금지). `${}` escape 미지원(모든 `${...}`는 템플릿).
- **렌더 결과는 항상 EphemeralMessageContent sanitize 통과**(static 포함). 마크다운 보존, 멘션/@everyone/@here/제어문자/길이만.
- 문법·참조 = validate(설치시점), 치환·보안 = run(런타임). TemplateError→AdapterError{BadRequest, 메시지 명시} 매핑.
- 완료 게이트: build/test/clippy(`--all-targets -- -D warnings`)/fmt. 완료 후 push.

---

## File Structure
- Create: `crates/automation-core/src/template.rs`
- Modify: `crates/automation-core/src/lib.rs` (template 모듈/재노출)
- Modify: `crates/automation-core/src/event.rs` (RuntimeContext + inputs, Copy 제거)
- Modify: `crates/automation-core/src/adapter.rs` (AdapterErrorKind::BadRequest)
- Modify: `crates/automation-core/src/run.rs` (RespondEphemeral 렌더)
- Modify: `crates/automation-core/src/validate.rs` (템플릿 사전검사)
- Create: `crates/automation-core/tests/template.rs` (validate + run 통합)

---

## Task 1: `template.rs` — 파싱·렌더·sanitize 엔진 (순수 TDD)

**Files:** template.rs(new), lib.rs(modify).

**Interfaces:**
- Produces: `SanitizeContext::EphemeralMessageContent`, `TemplateError`, `TemplateString::{parse, input_keys, render}`.

- [ ] **Step 1: `crates/automation-core/src/template.rs` 작성**

```rust
use std::collections::BTreeMap;

const EPHEMERAL_MAX_LEN: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SanitizeContext {
    EphemeralMessageContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateError {
    BadSyntax(String),
    UnsupportedVariable(String),
    MissingInput(String),
    TooLong { limit: usize, actual: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Input(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateString {
    segments: Vec<Segment>,
}

impl TemplateString {
    pub fn parse(source: &str) -> Result<TemplateString, TemplateError> {
        let mut segments = Vec::new();
        let mut rest = source;
        while let Some(start) = rest.find("${") {
            let literal = &rest[..start];
            if !literal.is_empty() {
                segments.push(Segment::Literal(literal.to_string()));
            }
            let after = &rest[start + 2..];
            let end = after
                .find('}')
                .ok_or_else(|| TemplateError::BadSyntax(source.to_string()))?;
            let expr = &after[..end];
            let key = expr
                .strip_prefix("input.")
                .ok_or_else(|| TemplateError::UnsupportedVariable(expr.to_string()))?;
            if key.is_empty() {
                return Err(TemplateError::BadSyntax(source.to_string()));
            }
            segments.push(Segment::Input(key.to_string()));
            rest = &after[end + 1..];
        }
        if !rest.is_empty() {
            segments.push(Segment::Literal(rest.to_string()));
        }
        Ok(TemplateString { segments })
    }

    pub fn input_keys(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Input(key) => Some(key.as_str()),
                Segment::Literal(_) => None,
            })
            .collect()
    }

    pub fn render(
        &self,
        inputs: &BTreeMap<String, String>,
        context: SanitizeContext,
    ) -> Result<String, TemplateError> {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Input(key) => {
                    let value = inputs
                        .get(key)
                        .ok_or_else(|| TemplateError::MissingInput(key.clone()))?;
                    out.push_str(value);
                }
            }
        }
        let sanitized = sanitize(&out, context);
        let limit = max_len(context);
        let actual = sanitized.chars().count();
        if actual > limit {
            return Err(TemplateError::TooLong { limit, actual });
        }
        Ok(sanitized)
    }
}

fn max_len(context: SanitizeContext) -> usize {
    match context {
        SanitizeContext::EphemeralMessageContent => EPHEMERAL_MAX_LEN,
    }
}

fn sanitize(input: &str, context: SanitizeContext) -> String {
    match context {
        SanitizeContext::EphemeralMessageContent => sanitize_message(input),
    }
}

fn sanitize_message(input: &str) -> String {
    let replaced = input
        .replace("@everyone", "@\u{200b}everyone")
        .replace("@here", "@\u{200b}here")
        .replace("<@", "<\u{200b}@")
        .replace("<#", "<\u{200b}#");
    replaced
        .chars()
        .filter(|character| *character == '\n' || !character.is_control())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn render(source: &str, pairs: &[(&str, &str)]) -> Result<String, TemplateError> {
        TemplateString::parse(source)?
            .render(&inputs(pairs), SanitizeContext::EphemeralMessageContent)
    }

    #[test]
    fn parse_literal_only() {
        let template = TemplateString::parse("hello world").unwrap();
        assert!(template.input_keys().is_empty());
    }

    #[test]
    fn parse_rejects_unclosed() {
        assert_eq!(
            TemplateString::parse("hi ${input.x").unwrap_err(),
            TemplateError::BadSyntax("hi ${input.x".to_string())
        );
    }

    #[test]
    fn parse_rejects_wrong_prefix() {
        assert_eq!(
            TemplateString::parse("${actor.id}").unwrap_err(),
            TemplateError::UnsupportedVariable("actor.id".to_string())
        );
    }

    #[test]
    fn parse_rejects_empty_key() {
        assert!(matches!(
            TemplateString::parse("${input.}").unwrap_err(),
            TemplateError::BadSyntax(_)
        ));
    }

    #[test]
    fn input_keys_extracted() {
        let template = TemplateString::parse("${input.a}-${input.b}").unwrap();
        assert_eq!(template.input_keys(), vec!["a", "b"]);
    }

    #[test]
    fn render_literal_unchanged() {
        assert_eq!(render("welcome", &[]).unwrap(), "welcome");
    }

    #[test]
    fn render_substitutes_inputs() {
        assert_eq!(
            render("room: ${input.name} / ${input.owner}", &[("name", "cozy"), ("owner", "kim")])
                .unwrap(),
            "room: cozy / kim"
        );
    }

    #[test]
    fn render_missing_input_errors() {
        assert_eq!(
            render("${input.x}", &[]).unwrap_err(),
            TemplateError::MissingInput("x".to_string())
        );
    }

    #[test]
    fn render_neutralizes_everyone_and_here() {
        let out = render("${input.x}", &[("x", "@everyone @here")]).unwrap();
        assert!(!out.contains("@everyone"));
        assert!(!out.contains("@here"));
    }

    #[test]
    fn render_neutralizes_mentions() {
        let out = render("${input.x}", &[("x", "<@123> <@&456> <#789>")]).unwrap();
        assert!(!out.contains("<@"));
        assert!(!out.contains("<#"));
    }

    #[test]
    fn render_preserves_markdown() {
        let out = render("${input.x}", &[("x", "**bold** _em_")]).unwrap();
        assert!(out.contains("**bold**"));
        assert!(out.contains("_em_"));
    }

    #[test]
    fn render_too_long_errors() {
        let long = "a".repeat(2001);
        assert!(matches!(
            render("${input.x}", &[("x", long.as_str())]).unwrap_err(),
            TemplateError::TooLong { .. }
        ));
    }
}
```

- [ ] **Step 2: `crates/automation-core/src/lib.rs` — template 모듈/재노출 추가**

모듈 선언에 `pub mod template;`(plan 다음, 알파벳 순), 재노출에 다음을 추가(알파벳 위치):

```rust
pub use template::{SanitizeContext, TemplateError, TemplateString};
```

최종 lib.rs 재노출 순서 참고: adapter, event, interpret, mock, plan, policy, run, **template**, validate.

- [ ] **Step 3: 테스트**

Run: `cargo test -p automation-core template`
Expected: template 순수 테스트 12개 PASS.

- [ ] **Step 4: 커밋**

```bash
git add crates/automation-core/src/template.rs crates/automation-core/src/lib.rs
git commit -m "feat(automation-core): template parse/render/sanitize engine"
```

---

## Task 2: 배선 — event · adapter · run · validate + 통합 테스트

**Files:** event.rs, adapter.rs, run.rs, validate.rs (modify), tests/template.rs (new).

- [ ] **Step 1: `event.rs` — RuntimeContext에 inputs (Copy 제거)**

`RuntimeContext` 구조체와 impl을 교체:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeContext {
    pub guild_id: GuildId,
    pub actor: UserId,
    pub inputs: BTreeMap<String, String>,
}

impl RuntimeContext {
    pub fn from_event(event: &RuntimeEvent) -> Self {
        let inputs = match &event.kind {
            EventKind::ModalSubmit { inputs, .. } => inputs.clone(),
            EventKind::ButtonClick { .. } => BTreeMap::new(),
        };
        Self {
            guild_id: event.guild_id,
            actor: event.actor,
            inputs,
        }
    }
}
```

(event.rs는 이미 `use std::collections::BTreeMap;` 있음.)

- [ ] **Step 2: `adapter.rs` — AdapterErrorKind::BadRequest 추가**

`AdapterErrorKind` enum에서 `Unsupported,` 다음 줄에 `BadRequest,` 추가:

```rust
pub enum AdapterErrorKind {
    Forbidden,
    NotFound,
    RateLimited,
    Network,
    Unsupported,
    BadRequest,
    Unknown,
}
```

- [ ] **Step 3: `run.rs` 전체 교체** (RespondEphemeral 렌더 + template_error 헬퍼)

```rust
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;

use crate::adapter::{AdapterError, AdapterErrorKind, DiscordMutationAdapter, InteractionResponder};
use crate::event::{RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{ActionPlan, PlannedAction};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

pub async fn run(
    context: &RuntimeContext,
    plan: &ActionPlan,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
) -> Result<(), AdapterError> {
    for step in &plan.steps {
        match step {
            PlannedAction::GrantRole { role, target } => {
                mutation
                    .grant_role(context.guild_id, *target, *role)
                    .await?;
            }
            PlannedAction::RespondEphemeral { content } => {
                let template = TemplateString::parse(content).map_err(template_error)?;
                let rendered = template
                    .render(&context.inputs, SanitizeContext::EphemeralMessageContent)
                    .map_err(template_error)?;
                responder.respond_ephemeral(rendered).await?;
            }
            PlannedAction::OpenModal(modal) => {
                responder.open_modal(modal).await?;
            }
        }
    }
    Ok(())
}

fn template_error(error: TemplateError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("template error: {error:?}"),
    )
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

- [ ] **Step 4: `validate.rs` 전체 교체** (템플릿 사전검사 추가)

```rust
use std::collections::{BTreeMap, BTreeSet};

use automation_state::{ActionSpec, InteractionRuleSet, TriggerSpec};
use desired_state::ResourceKey;
use resource_resolution::ResourceBindingMap;

use crate::template::TemplateString;

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
    BadTemplate { rule: String },
    InputTemplateInButtonRule { rule: String, input: String },
    UnknownTemplateInput { rule: String, modal: String, input: String },
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
    let mut modal_fields: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for modal in &ruleset.modals {
        if !modal_keys.insert(modal.key.clone()) {
            errors.push(ValidationError::DuplicateModalKey(modal.key.clone()));
        }
        let mut field_keys: BTreeSet<String> = BTreeSet::new();
        for field in &modal.fields {
            if !field_keys.insert(field.key.clone()) {
                errors.push(ValidationError::DuplicateModalFieldKey {
                    modal: modal.key.clone(),
                    field: field.key.clone(),
                });
            }
        }
        modal_fields.insert(modal.key.clone(), field_keys);
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
                    match TemplateString::parse(content) {
                        Err(_) => {
                            errors.push(ValidationError::BadTemplate {
                                rule: rule.key.clone(),
                            });
                        }
                        Ok(template) => {
                            for key in template.input_keys() {
                                match &rule.trigger {
                                    TriggerSpec::ButtonClick { .. } => {
                                        errors.push(ValidationError::InputTemplateInButtonRule {
                                            rule: rule.key.clone(),
                                            input: key.to_string(),
                                        });
                                    }
                                    TriggerSpec::ModalSubmit { modal } => {
                                        let known = modal_fields
                                            .get(modal)
                                            .is_some_and(|fields| fields.contains(key));
                                        if !known {
                                            errors.push(ValidationError::UnknownTemplateInput {
                                                rule: rule.key.clone(),
                                                modal: modal.clone(),
                                                input: key.to_string(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
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

- [ ] **Step 5: `crates/automation-core/tests/template.rs` 작성** (validate + run 통합)

```rust
use std::collections::BTreeMap;

use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, ResponderCall};
use automation_core::run::handle_event;
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
        key: "study_modal".to_string(),
        title: "Study".to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: "Room".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
        }],
    }
}

fn modal_rule(content: &str) -> InteractionRule {
    InteractionRule {
        key: "submit_rule".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "study_modal".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: content.to_string(),
        }],
    }
}

fn button_rule(content: &str) -> InteractionRule {
    InteractionRule {
        key: "click_rule".to_string(),
        trigger: TriggerSpec::ButtonClick {
            component: "b".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: content.to_string(),
        }],
    }
}

fn modal_submit(room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(9),
        kind: EventKind::ModalSubmit {
            modal: "study_modal".to_string(),
            inputs,
        },
    }
}

fn button_click() -> RuntimeEvent {
    RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(9),
        kind: EventKind::ButtonClick {
            component: "b".to_string(),
        },
    }
}

fn responded(event: &RuntimeEvent, rule: InteractionRule) -> String {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![rule],
    };
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    block_on(handle_event(
        event,
        &set,
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
    ))
    .unwrap();
    match responder.calls().into_iter().next().unwrap() {
        ResponderCall::RespondEphemeral { content } => content,
        other => panic!("expected RespondEphemeral, got {other:?}"),
    }
}

#[test]
fn button_input_template_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![automation_state::PanelSpec {
            key: "p".to_string(),
            channel: desired_state::ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![automation_state::ButtonSpec {
                key: "b".to_string(),
                label: "B".to_string(),
            }],
        }],
        modals: vec![],
        rules: vec![button_rule("${input.room_name}")],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::InputTemplateInButtonRule {
        rule: "click_rule".to_string(),
        input: "room_name".to_string(),
    }));
}

#[test]
fn modal_unknown_input_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![modal_rule("${input.ghost}")],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownTemplateInput {
        rule: "submit_rule".to_string(),
        modal: "study_modal".to_string(),
        input: "ghost".to_string(),
    }));
}

#[test]
fn modal_known_input_passes_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![modal_rule("room: ${input.room_name}")],
    };
    assert!(validate(&set, &ResourceBindingMap::default()).is_ok());
}

#[test]
fn bad_template_syntax_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![modal_rule("oops ${input.")],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::BadTemplate {
        rule: "submit_rule".to_string(),
    }));
}

#[test]
fn static_content_renders_unchanged() {
    assert_eq!(responded(&button_click(), button_rule("welcome")), "welcome");
}

#[test]
fn modal_input_rendered_into_response() {
    assert_eq!(
        responded(&modal_submit("cozy"), modal_rule("room: ${input.room_name}")),
        "room: cozy"
    );
}

#[test]
fn injected_mention_is_sanitized_in_response() {
    let out = responded(&modal_submit("@everyone"), modal_rule("${input.room_name}"));
    assert!(!out.contains("@everyone"));
}
```

- [ ] **Step 6: 테스트**

Run: `cargo test -p automation-core`
Expected: 전부 PASS. tests/template.rs 7개 + template 순수 12개 포함. **automation-runtime도 빌드:** `cargo build -p automation-runtime` 성공(RespondEphemeral 스키마 무변경).

- [ ] **Step 7: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): render+sanitize RespondEphemeral, validate template refs"
```

---

## Task 3: 워크스페이스 검증 게이트 + push

- [ ] **Step 1: 빌드** — `cargo build` (경고 0).
- [ ] **Step 2: 테스트** — `cargo test`. 신규 = template 순수 12 + tests/template 7 = 19. 기존 206 → 총 225.
- [ ] **Step 3: clippy** — `cargo clippy --all-targets -- -D warnings` (0).
- [ ] **Step 4: fmt** — `cargo fmt --all -- --check`.
- [ ] **Step 5: push** — `git push origin main`.

---

## Self-Review (스펙 대비)

- **D1~D8:** content String 유지·always-template·literal 렌더·`${input.x}`만·missing=error·항상 sanitize·스키마 무변경·동적 out ✅.
- **§4 파싱:** unclosed/wrong-prefix/empty-key 에러(template.rs) ✅.
- **§5 렌더:** 치환→sanitize→길이(run이 context.inputs로) ✅.
- **§6 sanitize:** @everyone/@here/멘션 ZWSP 무력화, 제어문자 제거(\n 유지), 마크다운 보존, 길이초과 error ✅.
- **§7 validate/run 경계:** validate가 parse+input_keys로 ButtonClick 금지·ModalSubmit field 존재 확인 / run이 렌더·sanitize ✅.
- **§8 에러:** TemplateError→AdapterError{BadRequest, "template error: ..."} ✅.
- **무변경:** RespondEphemeral 스키마·interpret·PlannedAction·automation-runtime·automation-state 무수정. RuntimeContext는 inputs 추가+Copy 제거(from_event만 사용 → 안전) ✅.
- **clippy:** `.is_some_and`, `let ... else` 불필요, `filter_map`, 주석 없음 ✅.
- **테스트:** 순수 12 + 통합 7 = 19. 기존 206 무변경 → 225.

---

## Codex 핸드오프 (권장 2청크)

- **청크 A** = Task 1 (template.rs 엔진 + 순수 테스트 12). 커밋 1개.
- **청크 B** = Task 2 + Task 3 (event/adapter/run/validate 배선 + 통합 테스트 + 게이트 + push). **Task2 Step6에서 `cargo build -p automation-runtime` 성공 필수**(RespondEphemeral 무변경 확인). 커밋 2개 + push.

**automation-runtime/automation-state 무수정.** 완료 보고: automation-core 테스트 수 + 전체(기대 225) + clippy/fmt + push 해시 + automation-runtime 무수정 + 이탈.
