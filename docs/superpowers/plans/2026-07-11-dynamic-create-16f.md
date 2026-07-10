# Phase 16f — Dynamic CreateChannel/CreateRole Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** 입력값 → 안전한 이름(ChannelName/RoleName sanitize) → CreateChannel/CreateRole action. 순수 코어 + Mock. created id 기록만(링킹 금지).

**Architecture:** automation-state에 ActionSpec 2종, automation-core에 sanitizer 2종·seam(default-unsupported)·PlannedAction 2종·CreatedResource·run 확장·policy enum. **automation-runtime 무수정.**

## Global Constraints
- **코드 주석 금지.** **Codex 구현.**
- **automation-runtime 무수정** — 새 seam(create_channel/create_role)은 **default-unsupported**(16c open_modal 패턴), TwilightMutationAdapter 무변경 컴파일.
- name은 always-template. `${input.<field_key>}`만. `${created.x}`는 파서가 자동 거부(UnsupportedVariable).
- CreateRole=권한없음, CreateChannel=공개텍스트, overwrite/permission/링킹/live 없음.
- 완료 게이트: build/test/clippy(`--all-targets -- -D warnings`)/fmt. 완료 후 push.

---

## File Structure
- Modify: `crates/automation-state/src/rule.rs` (ActionSpec + 기존 테스트 교체)
- Modify: `crates/automation-core/src/{template,adapter,plan,mock,interpret,run,validate,policy,lib}.rs`
- Create: `crates/automation-core/tests/create.rs`
- Modify: `crates/automation-core/tests/policy.rs` (PolicyFinding enum)

---

## Task 1: automation-state — ActionSpec::CreateChannel/CreateRole

- [ ] **Step 1: `rule.rs` — ActionSpec 확장**

`ActionSpec` enum의 `OpenModal { modal: String },` 다음에 추가:

```rust
    CreateChannel {
        name: String,
    },
    CreateRole {
        name: String,
    },
```

- [ ] **Step 2: `rule.rs` — 기존 테스트 교체** (create_channel이 이제 유효 타입이므로 미지원 타입을 post_panel로)

`unknown_action_type_is_rejected` 테스트를 교체:

```rust
    #[test]
    fn unknown_action_type_is_rejected() {
        let json = r#"{"type":"post_panel","channel":"x"}"#;
        assert!(serde_json::from_str::<ActionSpec>(json).is_err());
    }
```

- [ ] **Step 3: 테스트 + 커밋**

Run: `cargo test -p automation-state` (기존 통과 유지)
```bash
git add crates/automation-state/src/rule.rs
git commit -m "feat(automation-state): CreateChannel/CreateRole action specs"
```

---

## Task 2: automation-core — sanitizer · seam · plan · mock

- [ ] **Step 1: `template.rs` — SanitizeContext/TemplateError 확장 + fallible sanitize + 2 sanitizer**

`SanitizeContext` enum을 교체:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SanitizeContext {
    EphemeralMessageContent,
    ChannelName,
    RoleName,
}
```

`TemplateError` enum의 `TooLong {...},` 다음에 추가:

```rust
    EmptyAfterSanitize,
```

상수에 추가(파일 상단 `const EPHEMERAL_MAX_LEN` 아래):

```rust
const NAME_MAX_LEN: usize = 100;
```

`render`의 sanitize 호출을 fallible로 — `let sanitized = sanitize(&out, context);` 를 다음으로:

```rust
        let sanitized = sanitize(&out, context)?;
```

`max_len` 함수를 교체:

```rust
fn max_len(context: SanitizeContext) -> usize {
    match context {
        SanitizeContext::EphemeralMessageContent => EPHEMERAL_MAX_LEN,
        SanitizeContext::ChannelName | SanitizeContext::RoleName => NAME_MAX_LEN,
    }
}
```

`sanitize` 함수를 교체(Result + empty 검사 + 2 sanitizer 분기):

```rust
fn sanitize(input: &str, context: SanitizeContext) -> Result<String, TemplateError> {
    let result = match context {
        SanitizeContext::EphemeralMessageContent => sanitize_message(input),
        SanitizeContext::ChannelName => sanitize_channel_name(input),
        SanitizeContext::RoleName => sanitize_role_name(input),
    };
    if result.is_empty() {
        Err(TemplateError::EmptyAfterSanitize)
    } else {
        Ok(result)
    }
}
```

`sanitize_message` 함수 다음에 2 sanitizer 추가:

```rust
fn sanitize_channel_name(input: &str) -> String {
    let mut result = String::new();
    for character in input.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            result.push(character);
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    result.trim_matches('-').to_string()
}

fn sanitize_role_name(input: &str) -> String {
    let neutralized = input
        .replace("@everyone", "@\u{200b}everyone")
        .replace("@here", "@\u{200b}here")
        .replace("<@", "<\u{200b}@")
        .replace("<#", "<\u{200b}#");
    let cleaned: String = neutralized
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}
```

- [ ] **Step 2: `template.rs` — sanitizer 순수 테스트 추가**

`#[cfg(test)] mod tests`의 `render_too_long_errors` 테스트 다음에 추가:

```rust
    #[test]
    fn parse_rejects_created_variable() {
        assert_eq!(
            TemplateString::parse("${created.channel.id}").unwrap_err(),
            TemplateError::UnsupportedVariable("created.channel.id".to_string())
        );
    }

    fn channel(input: &str) -> Result<String, TemplateError> {
        TemplateString::parse("${input.x}")?
            .render(&inputs(&[("x", input)]), SanitizeContext::ChannelName)
    }

    fn role(input: &str) -> Result<String, TemplateError> {
        TemplateString::parse("${input.x}")?
            .render(&inputs(&[("x", input)]), SanitizeContext::RoleName)
    }

    #[test]
    fn channel_name_spaces_to_hyphens() {
        assert_eq!(channel("study room").unwrap(), "study-room");
    }

    #[test]
    fn channel_name_lowercased() {
        assert_eq!(channel("Study Room 1").unwrap(), "study-room-1");
    }

    #[test]
    fn channel_name_removes_invalid_chars() {
        assert_eq!(channel("study!@#room").unwrap(), "study-room");
    }

    #[test]
    fn channel_name_empty_after_sanitize_errors() {
        assert_eq!(channel("수학").unwrap_err(), TemplateError::EmptyAfterSanitize);
        assert_eq!(channel("!!!!").unwrap_err(), TemplateError::EmptyAfterSanitize);
    }

    #[test]
    fn channel_name_too_long_errors() {
        assert!(matches!(
            channel(&"a".repeat(101)).unwrap_err(),
            TemplateError::TooLong { .. }
        ));
    }

    #[test]
    fn role_name_keeps_hangul() {
        assert_eq!(role("수학 스터디 멤버").unwrap(), "수학 스터디 멤버");
    }

    #[test]
    fn role_name_neutralizes_everyone() {
        let out = role("@everyone 멤버").unwrap();
        assert!(!out.contains("@everyone"));
        assert!(out.contains("멤버"));
    }

    #[test]
    fn role_name_too_long_errors() {
        assert!(matches!(
            role(&"가".repeat(101)).unwrap_err(),
            TemplateError::TooLong { .. }
        ));
    }
```

- [ ] **Step 3: `adapter.rs` — CreateChannelSpec/CreateRoleSpec + seam default-unsupported**

`use discord_model::{GuildId, RoleId, UserId};` 를 다음으로:

```rust
use discord_model::{ChannelId, GuildId, RoleId, UserId};
```

`AdapterError` impl 다음, `DiscordMutationAdapter` trait 앞에 spec 추가:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateChannelSpec {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateRoleSpec {
    pub name: String,
}
```

`DiscordMutationAdapter` trait을 교체(grant_role 다음에 default-unsupported 2종):

```rust
#[allow(async_fn_in_trait)]
pub trait DiscordMutationAdapter {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError>;

    async fn create_channel(
        &self,
        _guild: GuildId,
        _spec: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "create_channel is not supported",
        ))
    }

    async fn create_role(
        &self,
        _guild: GuildId,
        _spec: CreateRoleSpec,
    ) -> Result<RoleId, AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "create_role is not supported",
        ))
    }
}
```

- [ ] **Step 4: `plan.rs` 전체 교체** (PlannedAction 2종 + CreatedResource)

```rust
use automation_state::ModalFieldSpec;
use discord_model::{ChannelId, RoleId, UserId};

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
    CreateChannel { name: String },
    CreateRole { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreatedResource {
    Channel {
        action_index: usize,
        name: String,
        id: ChannelId,
    },
    Role {
        action_index: usize,
        name: String,
        id: RoleId,
    },
}
```

- [ ] **Step 5: `mock.rs` — create_* 오버라이드 + MutationCall 2종 + id 카운터**

파일 상단 import 조정 — `use std::sync::Mutex;` 를 다음으로:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
```

`use discord_model::{GuildId, RoleId, UserId};` 를 다음으로:

```rust
use discord_model::{ChannelId, GuildId, RoleId, UserId};
```

`use crate::adapter::{...};` 를 다음으로:

```rust
use crate::adapter::{
    AdapterError, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter, InteractionResponder,
};
```

`MutationCall` enum에 변형 추가:

```rust
pub enum MutationCall {
    GrantRole {
        guild: GuildId,
        member: UserId,
        role: RoleId,
    },
    CreateChannel {
        guild: GuildId,
        name: String,
    },
    CreateRole {
        guild: GuildId,
        name: String,
    },
}
```

`MockMutationAdapter` 구조체에 필드 추가:

```rust
#[derive(Default)]
pub struct MockMutationAdapter {
    calls: Mutex<Vec<MutationCall>>,
    fail: Option<AdapterError>,
    next_id: AtomicU64,
}
```

`failing`은 `next_id: AtomicU64::new(0),`를 필드에 추가(구조체 리터럴):

```rust
    pub fn failing(error: AdapterError) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail: Some(error),
            next_id: AtomicU64::new(0),
        }
    }
```

`impl DiscordMutationAdapter for MockMutationAdapter`의 `grant_role` 다음에 추가:

```rust
    async fn create_channel(
        &self,
        guild: GuildId,
        spec: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        self.calls.lock().unwrap().push(MutationCall::CreateChannel {
            guild,
            name: spec.name,
        });
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        Ok(ChannelId(800_000 + self.next_id.fetch_add(1, Ordering::SeqCst)))
    }

    async fn create_role(
        &self,
        guild: GuildId,
        spec: CreateRoleSpec,
    ) -> Result<RoleId, AdapterError> {
        self.calls.lock().unwrap().push(MutationCall::CreateRole {
            guild,
            name: spec.name,
        });
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        Ok(RoleId(800_000 + self.next_id.fetch_add(1, Ordering::SeqCst)))
    }
```

- [ ] **Step 6: `lib.rs` — 재노출 추가**

`pub use adapter::{...}` 를 다음으로:

```rust
pub use adapter::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    InteractionResponder,
};
```

`pub use plan::{ActionPlan, ModalPresentation, PlannedAction};` 를 다음으로:

```rust
pub use plan::{ActionPlan, CreatedResource, ModalPresentation, PlannedAction};
```

`pub use policy::{analyze, privileged_mask, PolicyFinding};` 를 다음으로(Task 3에서 DynamicAction 추가):

```rust
pub use policy::{analyze, privileged_mask, DynamicAction, PolicyFinding};
```

- [ ] **Step 7: 빌드 + 테스트 + 커밋**

Run: `cargo test -p automation-core template` (기존 20 + 신규 9 = 29 통과) / `cargo build -p automation-core` / **`cargo build -p automation-runtime`(default seam 상속 확인)**
```bash
git add crates/automation-core
git commit -m "feat(automation-core): channel/role sanitizers, create seams, CreatedResource"
```

---

## Task 3: automation-core — interpret · run · validate · policy + 테스트

- [ ] **Step 1: `interpret.rs` 전체 교체** (청크 A의 no-op arm을 실제 arm으로 — 중복 추가 금지)

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
                let spec = ruleset
                    .modals
                    .iter()
                    .find(|candidate| candidate.key == *modal)?;
                steps.push(PlannedAction::OpenModal(ModalPresentation {
                    key: spec.key.clone(),
                    title: spec.title.clone(),
                    fields: spec.fields.clone(),
                }));
            }
            ActionSpec::CreateChannel { name } => {
                steps.push(PlannedAction::CreateChannel { name: name.clone() });
            }
            ActionSpec::CreateRole { name } => {
                steps.push(PlannedAction::CreateRole { name: name.clone() });
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
        (
            TriggerSpec::ModalSubmit { modal },
            EventKind::ModalSubmit {
                modal: submitted, ..
            },
        ) => modal == submitted,
        _ => false,
    }
}
```

- [ ] **Step 2: `run.rs` 전체 교체** (Vec 반환 + create arm + enumerate)

```rust
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;

use crate::adapter::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    InteractionResponder,
};
use crate::event::{RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{ActionPlan, CreatedResource, PlannedAction};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

pub async fn run(
    context: &RuntimeContext,
    plan: &ActionPlan,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
) -> Result<Vec<CreatedResource>, AdapterError> {
    let mut created = Vec::new();
    for (action_index, step) in plan.steps.iter().enumerate() {
        match step {
            PlannedAction::GrantRole { role, target } => {
                mutation
                    .grant_role(context.guild_id, *target, *role)
                    .await?;
            }
            PlannedAction::RespondEphemeral { content } => {
                let rendered = render(content, context, SanitizeContext::EphemeralMessageContent)?;
                responder.respond_ephemeral(rendered).await?;
            }
            PlannedAction::OpenModal(modal) => {
                responder.open_modal(modal).await?;
            }
            PlannedAction::CreateChannel { name } => {
                let rendered = render(name, context, SanitizeContext::ChannelName)?;
                let id = mutation
                    .create_channel(context.guild_id, CreateChannelSpec { name: rendered.clone() })
                    .await?;
                created.push(CreatedResource::Channel {
                    action_index,
                    name: rendered,
                    id,
                });
            }
            PlannedAction::CreateRole { name } => {
                let rendered = render(name, context, SanitizeContext::RoleName)?;
                let id = mutation
                    .create_role(context.guild_id, CreateRoleSpec { name: rendered.clone() })
                    .await?;
                created.push(CreatedResource::Role {
                    action_index,
                    name: rendered,
                    id,
                });
            }
        }
    }
    Ok(created)
}

fn render(
    source: &str,
    context: &RuntimeContext,
    sanitize: SanitizeContext,
) -> Result<String, AdapterError> {
    TemplateString::parse(source)
        .and_then(|template| template.render(&context.inputs, sanitize))
        .map_err(template_error)
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

- [ ] **Step 3: `validate.rs` — check_template 헬퍼 + create arm**

`ValidationError` enum은 16e 그대로 유지(BadTemplate/InputTemplateInButtonRule/UnknownTemplateInput 이미 있음). action match에서 RespondEphemeral의 인라인 템플릿 검사를 헬퍼로 빼고 create에도 적용.

action match(`for action in &rule.actions`)를 다음으로 교체:

```rust
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
                    check_template(&mut errors, rule, &modal_fields, content);
                }
                ActionSpec::OpenModal { modal } => {
                    if !modal_keys.contains(modal) {
                        errors.push(ValidationError::UnknownModalRef {
                            rule: rule.key.clone(),
                            modal: modal.clone(),
                        });
                    }
                }
                ActionSpec::CreateChannel { name } => {
                    check_template(&mut errors, rule, &modal_fields, name);
                }
                ActionSpec::CreateRole { name } => {
                    check_template(&mut errors, rule, &modal_fields, name);
                }
            }
        }
```

파일 끝(`validate` 함수 닫는 `}` 다음)에 헬퍼 추가 + import에 TemplateString/InteractionRule 확인:

```rust
fn check_template(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    modal_fields: &BTreeMap<String, BTreeSet<String>>,
    content: &str,
) {
    let template = match TemplateString::parse(content) {
        Ok(template) => template,
        Err(_) => {
            errors.push(ValidationError::BadTemplate {
                rule: rule.key.clone(),
            });
            return;
        }
    };
    for key in template.input_keys() {
        match &rule.trigger {
            TriggerSpec::ButtonClick { .. } => {
                errors.push(ValidationError::InputTemplateInButtonRule {
                    rule: rule.key.clone(),
                    input: key.to_string(),
                });
            }
            TriggerSpec::ModalSubmit { modal } => {
                if !modal_fields
                    .get(modal)
                    .is_some_and(|fields| fields.contains(key))
                {
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
```

validate.rs import에 `InteractionRule` 추가: `use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};`.

- [ ] **Step 4: `policy.rs` 전체 교체** (PolicyFinding enum + DynamicResourceCreation)

```rust
use std::collections::BTreeMap;

use automation_state::{ActionSpec, InteractionRuleSet};
use desired_state::ResourceKey;
use discord_model::Permissions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyFinding {
    PrivilegedRoleGrant { rule: String, role: ResourceKey },
    DynamicResourceCreation { rule: String, action: DynamicAction },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicAction {
    CreateChannel,
    CreateRole,
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
            match action {
                ActionSpec::GrantRole { role, .. } => {
                    if roles.get(role).is_some_and(|perms| perms.intersects(mask)) {
                        findings.push(PolicyFinding::PrivilegedRoleGrant {
                            rule: rule.key.clone(),
                            role: role.clone(),
                        });
                    }
                }
                ActionSpec::CreateChannel { .. } => {
                    findings.push(PolicyFinding::DynamicResourceCreation {
                        rule: rule.key.clone(),
                        action: DynamicAction::CreateChannel,
                    });
                }
                ActionSpec::CreateRole { .. } => {
                    findings.push(PolicyFinding::DynamicResourceCreation {
                        rule: rule.key.clone(),
                        action: DynamicAction::CreateRole,
                    });
                }
                ActionSpec::RespondEphemeral { .. } | ActionSpec::OpenModal { .. } => {}
            }
        }
    }
    findings
}
```

- [ ] **Step 4b: `lib.rs` — DynamicAction 재노출 추가** (청크 A에서 뺐던 것 — DynamicAction이 이제 존재)

`pub use policy::{analyze, privileged_mask, PolicyFinding};` 를 다음으로:

```rust
pub use policy::{analyze, privileged_mask, DynamicAction, PolicyFinding};
```

- [ ] **Step 5: `tests/policy.rs` 전체 교체** (enum 반영 + dynamic 테스트)

```rust
use std::collections::BTreeMap;

use automation_core::policy::{analyze, DynamicAction, PolicyFinding};
use automation_state::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::Permissions;

fn grant_rule(key: &str, role: &str) -> InteractionRule {
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
    roles.insert(ResourceKey("admin".to_string()), Permissions::ADMINISTRATOR);
    roles.insert(
        ResourceKey("verified".to_string()),
        Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
    );
    roles
}

fn set(rule: InteractionRule) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![rule],
    }
}

#[test]
fn granting_privileged_role_is_flagged() {
    let findings = analyze(&set(grant_rule("r1", "admin")), &roles());
    assert_eq!(
        findings,
        vec![PolicyFinding::PrivilegedRoleGrant {
            rule: "r1".to_string(),
            role: ResourceKey("admin".to_string()),
        }]
    );
}

#[test]
fn granting_ordinary_role_is_allowed() {
    assert!(analyze(&set(grant_rule("r1", "verified")), &roles()).is_empty());
}

#[test]
fn create_channel_is_flagged() {
    let rule = InteractionRule {
        key: "r1".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "m".to_string(),
        },
        actions: vec![ActionSpec::CreateChannel {
            name: "study-x".to_string(),
        }],
    };
    assert_eq!(
        analyze(&set(rule), &roles()),
        vec![PolicyFinding::DynamicResourceCreation {
            rule: "r1".to_string(),
            action: DynamicAction::CreateChannel,
        }]
    );
}

#[test]
fn create_role_is_flagged() {
    let rule = InteractionRule {
        key: "r1".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "m".to_string(),
        },
        actions: vec![ActionSpec::CreateRole {
            name: "member".to_string(),
        }],
    };
    assert_eq!(
        analyze(&set(rule), &roles()),
        vec![PolicyFinding::DynamicResourceCreation {
            rule: "r1".to_string(),
            action: DynamicAction::CreateRole,
        }]
    );
}
```

- [ ] **Step 6: `tests/create.rs` 작성** (interpret/run/validate 통합)

```rust
use std::collections::BTreeMap;

use automation_core::adapter::AdapterErrorKind;
use automation_core::event::{EventKind, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{ActionPlan, CreatedResource, PlannedAction};
use automation_core::run::{handle_event, run};
use automation_core::validate::{validate, ValidationError};
use automation_core::RuntimeContext;
use automation_state::{
    ActionSpec, ButtonSpec, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle,
    ModalSpec, PanelSpec, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, RoleId, UserId};
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

fn submit_rule(actions: Vec<ActionSpec>) -> InteractionRule {
    InteractionRule {
        key: "submit".to_string(),
        trigger: TriggerSpec::ModalSubmit {
            modal: "study_modal".to_string(),
        },
        actions,
    }
}

fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![submit_rule(actions)],
    }
}

fn submit(room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(5),
        actor: UserId(9),
        kind: EventKind::ModalSubmit {
            modal: "study_modal".to_string(),
            inputs,
        },
    }
}

fn run_calls(event: &RuntimeEvent, actions: Vec<ActionSpec>) -> Vec<MutationCall> {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    block_on(handle_event(
        event,
        &ruleset(actions),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
    ))
    .unwrap();
    mutation.calls()
}

#[test]
fn create_channel_renders_name() {
    let calls = run_calls(
        &submit("cozy corner"),
        vec![ActionSpec::CreateChannel {
            name: "study-${input.room_name}".to_string(),
        }],
    );
    assert_eq!(
        calls,
        vec![MutationCall::CreateChannel {
            guild: GuildId(5),
            name: "study-cozy-corner".to_string(),
        }]
    );
}

#[test]
fn create_role_renders_name() {
    let calls = run_calls(
        &submit("코딩"),
        vec![ActionSpec::CreateRole {
            name: "${input.room_name} 멤버".to_string(),
        }],
    );
    assert_eq!(
        calls,
        vec![MutationCall::CreateRole {
            guild: GuildId(5),
            name: "코딩 멤버".to_string(),
        }]
    );
}

#[test]
fn create_channel_missing_input_errors() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let mut inputs = BTreeMap::new();
    inputs.insert("other".to_string(), "x".to_string());
    let event = RuntimeEvent {
        guild_id: GuildId(5),
        actor: UserId(9),
        kind: EventKind::ModalSubmit {
            modal: "study_modal".to_string(),
            inputs,
        },
    };
    let result = block_on(handle_event(
        &event,
        &ruleset(vec![ActionSpec::CreateChannel {
            name: "study-${input.room_name}".to_string(),
        }]),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
    ));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::BadRequest);
}

#[test]
fn created_ids_recorded_in_run_result() {
    let context = RuntimeContext::from_event(&submit("cozy"));
    let plan = ActionPlan {
        steps: vec![
            PlannedAction::CreateChannel {
                name: "study-${input.room_name}".to_string(),
            },
            PlannedAction::CreateRole {
                name: "${input.room_name} 멤버".to_string(),
            },
        ],
    };
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let created = block_on(run(&context, &plan, &mutation, &responder)).unwrap();
    assert_eq!(
        created,
        vec![
            CreatedResource::Channel {
                action_index: 0,
                name: "study-cozy".to_string(),
                id: ChannelId(800_000),
            },
            CreatedResource::Role {
                action_index: 1,
                name: "cozy 멤버".to_string(),
                id: RoleId(800_001),
            },
        ]
    );
}

#[test]
fn button_rule_create_input_template_fails_validate() {
    let set = InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "p".to_string(),
            channel: ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![ButtonSpec {
                key: "b".to_string(),
                label: "B".to_string(),
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "click".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "b".to_string(),
            },
            actions: vec![ActionSpec::CreateChannel {
                name: "study-${input.room_name}".to_string(),
            }],
        }],
    };
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::InputTemplateInButtonRule {
        rule: "click".to_string(),
        input: "room_name".to_string(),
    }));
}
```

- [ ] **Step 7: 빌드 + 테스트 + 커밋**

Run: `cargo test -p automation-core` (전부 통과)
```bash
git add crates/automation-core
git commit -m "feat(automation-core): create action interpret/run, template validation, dynamic policy"
```

---

## Task 4: 검증 게이트 + push

- [ ] Step 1: `cargo build` (경고 0).
- [ ] Step 2: `cargo test` → 전체 244 (기존 225 + 신규 19: template 9 + create 6 + policy 2 + mock... 재계산은 실제 실행값 우선).
- [ ] Step 3: `cargo clippy --all-targets -- -D warnings` (0).
- [ ] Step 4: `cargo fmt --all -- --check`.
- [ ] Step 5: `git push origin main`.

---

## Self-Review (스펙 대비)
- **D:** CreateChannel/CreateRole(name always-template) · ChannelName/RoleName sanitizer · seam default-unsupported · created id 기록만(링킹은 파서가 `${created}` 자동 거부) · CreateRole 권한없음/CreateChannel 공개 · dynamic policy ✅.
- **automation-runtime 무수정**: create seam default-unsupported → TwilightMutationAdapter 무변경(Task2 Step7 확인) ✅.
- **run→Vec<CreatedResource>**, handle_event discard(HandleOutcome 무변경 → 기존 테스트 유지) ✅.
- **PolicyFinding enum** + 기존 policy 테스트 2개 갱신 ✅.
- **sanitize fallible**(EmptyAfterSanitize), render 내부라 시그니처 무변경 ✅.
- **rule.rs 기존 테스트 교체**(create_channel→post_panel) ✅.
- **clippy:** `is_some_and`, `flat_map(char::to_lowercase)`, `let _ = (guild, spec)`(unused param), enum 매치 exhaustive, 주석 없음 ✅.

## Codex 핸드오프 (권장 2청크)
- **청크 A** = Task 1 + Task 2. **Task2 Step7에서 `cargo build -p automation-runtime` 성공 필수**. 커밋 2개.
- **청크 B** = Task 3 + Task 4. 커밋 2개 + push.
**automation-runtime 무수정.** 완료 보고: automation-core 테스트 수 + 전체 + clippy/fmt + push 해시 + runtime 무수정 + 이탈.
