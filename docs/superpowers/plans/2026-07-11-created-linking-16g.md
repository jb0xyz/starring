# Phase 16g — Created Resource Linking Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** CreateRole/CreateChannel에 key, run이 created role id를 RuntimeBindings에 저장, GrantRole이 typed RoleRef로 created role 참조. 순수 Mock.

**Architecture:** automation-state에 RoleRef + ActionSpec key. automation-core에 PlannedRole·RuntimeBindings·validate order 추적·policy notice. **automation-runtime 무수정**(grant_role seam 불변; created 해소는 core). GrantRole.role: ResourceKey→RoleRef 스키마 변경(untagged serde, JSON 호환; Rust fixture는 컴파일러 가이드로 갱신).

## Global Constraints
- **코드 주석 금지.** **Codex 구현.**
- **automation-runtime 무수정** — grant_role/create_* seam 시그니처 불변.
- **typed reference만** — `${created.x.id}` 문자열은 16e 파서가 자동 거부(변경 없음).
- created ref는 **앞선 CreateRole key만**(forward ref 금지, 순서 실행).
- 완료 게이트: build/test/clippy(`--all-targets -- -D warnings`)/fmt. 완료 후 push.

---

## Task 1: automation-state — RoleRef + ActionSpec key

- [ ] **Step 1: `rule.rs` — RoleRef 추가 + ActionSpec 변경**

`ActionSpec` enum 앞(ActionTarget 앞 또는 뒤 아무 곳)에 RoleRef 추가:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoleRef {
    Existing(ResourceKey),
    Created { created: String },
}
```

`ActionSpec` enum을 교체(GrantRole.role→RoleRef, Create*에 key):

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionSpec {
    GrantRole {
        role: RoleRef,
        target: ActionTarget,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal {
        modal: String,
    },
    CreateChannel {
        key: String,
        name: String,
    },
    CreateRole {
        key: String,
        name: String,
    },
}
```

- [ ] **Step 2: `lib.rs`(automation-state) — RoleRef 재노출**

`pub use rule::{ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, TriggerSpec};` 를 다음으로:

```rust
pub use rule::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, RoleRef, TriggerSpec,
};
```

- [ ] **Step 3: `rule.rs` 테스트 fixture 갱신 (컴파일러 가이드)**

`#[cfg(test)]` 모듈의 fixture를 새 스키마에 맞춘다(컴파일러가 매 site를 잡음):
- `ActionSpec::GrantRole { role: ResourceKey("x".into()), target }` → `role: RoleRef::Existing(ResourceKey("x".into()))`.
- `ActionSpec::CreateChannel { name }` → `{ key: "ch".to_string(), name }`; `CreateRole { name }` → `{ key: "role".to_string(), name }`.
- test 모듈 import에 `RoleRef` 필요 시 `use super::*` 로 커버(이미 있음).

- [ ] **Step 4: 테스트 + 커밋**

Run: `cargo test -p automation-state`
```bash
git add crates/automation-state
git commit -m "feat(automation-state): RoleRef + action output keys"
```

---

## Task 2: automation-core — PlannedRole · interpret · run(RuntimeBindings) · validate · policy

- [ ] **Step 1: `plan.rs` 전체 교체** (PlannedRole + PlannedAction key)

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
pub enum PlannedRole {
    Resolved(RoleId),
    Created(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedAction {
    GrantRole { role: PlannedRole, target: UserId },
    RespondEphemeral { content: String },
    OpenModal(ModalPresentation),
    CreateChannel { key: String, name: String },
    CreateRole { key: String, name: String },
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

- [ ] **Step 2: `interpret.rs` 전체 교체** (GrantRole RoleRef 처리 + create key 유지)

```rust
use automation_state::{ActionSpec, ActionTarget, InteractionRuleSet, RoleRef, TriggerSpec};
use resource_resolution::ResourceBindingMap;

use crate::event::{EventKind, RuntimeEvent};
use crate::plan::{ActionPlan, ModalPresentation, PlannedAction, PlannedRole};

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
                let planned_role = match role {
                    RoleRef::Existing(key) => {
                        PlannedRole::Resolved(*bindings.role_bindings.get(key)?)
                    }
                    RoleRef::Created { created } => PlannedRole::Created(created.clone()),
                };
                let target_id = match target {
                    ActionTarget::Actor => event.actor,
                };
                steps.push(PlannedAction::GrantRole {
                    role: planned_role,
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
            ActionSpec::CreateChannel { key, name } => {
                steps.push(PlannedAction::CreateChannel {
                    key: key.clone(),
                    name: name.clone(),
                });
            }
            ActionSpec::CreateRole { key, name } => {
                steps.push(PlannedAction::CreateRole {
                    key: key.clone(),
                    name: name.clone(),
                });
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

- [ ] **Step 3: `run.rs` 전체 교체** (RuntimeBindings + Created 해소; created_roles만, channel은 CreatedResource로)

```rust
use std::collections::BTreeMap;

use automation_state::InteractionRuleSet;
use discord_model::RoleId;
use resource_resolution::ResourceBindingMap;

use crate::adapter::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    InteractionResponder,
};
use crate::event::{RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{ActionPlan, CreatedResource, PlannedAction, PlannedRole};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

#[derive(Default)]
struct RuntimeBindings {
    created_roles: BTreeMap<String, RoleId>,
}

pub async fn run(
    context: &RuntimeContext,
    plan: &ActionPlan,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
) -> Result<Vec<CreatedResource>, AdapterError> {
    let mut created = Vec::new();
    let mut runtime = RuntimeBindings::default();
    for (action_index, step) in plan.steps.iter().enumerate() {
        match step {
            PlannedAction::GrantRole { role, target } => {
                let role_id = match role {
                    PlannedRole::Resolved(id) => *id,
                    PlannedRole::Created(key) => *runtime
                        .created_roles
                        .get(key)
                        .ok_or_else(|| unresolved_created_role(key))?,
                };
                mutation
                    .grant_role(context.guild_id, *target, role_id)
                    .await?;
            }
            PlannedAction::RespondEphemeral { content } => {
                let rendered = render(content, context, SanitizeContext::EphemeralMessageContent)?;
                responder.respond_ephemeral(rendered).await?;
            }
            PlannedAction::OpenModal(modal) => {
                responder.open_modal(modal).await?;
            }
            PlannedAction::CreateChannel { name, .. } => {
                let rendered = render(name, context, SanitizeContext::ChannelName)?;
                let id = mutation
                    .create_channel(
                        context.guild_id,
                        CreateChannelSpec {
                            name: rendered.clone(),
                        },
                    )
                    .await?;
                created.push(CreatedResource::Channel {
                    action_index,
                    name: rendered,
                    id,
                });
            }
            PlannedAction::CreateRole { key, name } => {
                let rendered = render(name, context, SanitizeContext::RoleName)?;
                let id = mutation
                    .create_role(
                        context.guild_id,
                        CreateRoleSpec {
                            name: rendered.clone(),
                        },
                    )
                    .await?;
                runtime.created_roles.insert(key.clone(), id);
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

fn unresolved_created_role(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved created role: {key}"),
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

> `CreateChannel { name, .. }` — 16g는 channel key를 소비 안 함(패턴에서 무시). 스키마엔 유지(16h에서 소비). channel id는 CreatedResource에 기록됨.

- [ ] **Step 4: `validate.rs` — GrantRole RoleRef + order 기반 create key 검사**

`ValidationError` enum 끝(마지막 변형 다음)에 추가:

```rust
    DuplicateActionKey {
        rule: String,
        key: String,
    },
    UnknownCreatedRoleRef {
        rule: String,
        key: String,
    },
    CreatedRoleRefTypeMismatch {
        rule: String,
        key: String,
    },
```

import에 `RoleRef` 추가: `use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, RoleRef, TriggerSpec};`

action match(`for action in &rule.actions { match action {...} }`)를 다음으로 교체(순서 추적 위해 rule 루프 안에서 `created` 맵 선언):

```rust
        let mut created: BTreeMap<String, CreatedKind> = BTreeMap::new();
        for action in &rule.actions {
            match action {
                ActionSpec::GrantRole { role, .. } => match role {
                    RoleRef::Existing(key) => {
                        if !bindings.role_bindings.contains_key(key) {
                            errors.push(ValidationError::UnknownRoleRef {
                                rule: rule.key.clone(),
                                role: key.clone(),
                            });
                        }
                    }
                    RoleRef::Created { created: key } => match created.get(key) {
                        None => errors.push(ValidationError::UnknownCreatedRoleRef {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        }),
                        Some(CreatedKind::Channel) => {
                            errors.push(ValidationError::CreatedRoleRefTypeMismatch {
                                rule: rule.key.clone(),
                                key: key.clone(),
                            })
                        }
                        Some(CreatedKind::Role) => {}
                    },
                },
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
                ActionSpec::CreateChannel { key, name } => {
                    if created.insert(key.clone(), CreatedKind::Channel).is_some() {
                        errors.push(ValidationError::DuplicateActionKey {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        });
                    }
                    check_template(&mut errors, rule, &modal_fields, name);
                }
                ActionSpec::CreateRole { key, name } => {
                    if created.insert(key.clone(), CreatedKind::Role).is_some() {
                        errors.push(ValidationError::DuplicateActionKey {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        });
                    }
                    check_template(&mut errors, rule, &modal_fields, name);
                }
            }
        }
```

파일 끝(check_template 함수 다음)에 CreatedKind 추가:

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum CreatedKind {
    Role,
    Channel,
}
```

- [ ] **Step 5: `policy.rs` — GrantRole RoleRef + CreatedResourceReference**

`PolicyFinding` enum에 변형 추가:

```rust
    CreatedResourceReference { rule: String },
```

`use automation_state::{ActionSpec, InteractionRuleSet};` 를 `use automation_state::{ActionSpec, InteractionRuleSet, RoleRef};` 로.

`analyze`의 GrantRole arm을 다음으로 교체:

```rust
                ActionSpec::GrantRole { role, .. } => match role {
                    RoleRef::Existing(key) => {
                        if roles.get(key).is_some_and(|perms| perms.intersects(mask)) {
                            findings.push(PolicyFinding::PrivilegedRoleGrant {
                                rule: rule.key.clone(),
                                role: key.clone(),
                            });
                        }
                    }
                    RoleRef::Created { .. } => {
                        findings.push(PolicyFinding::CreatedResourceReference {
                            rule: rule.key.clone(),
                        });
                    }
                },
```

- [ ] **Step 6: `lib.rs`(automation-core) — PlannedRole 재노출**

`pub use plan::{ActionPlan, CreatedResource, ModalPresentation, PlannedAction};` 를:

```rust
pub use plan::{ActionPlan, CreatedResource, ModalPresentation, PlannedAction, PlannedRole};
```

- [ ] **Step 7: 빌드 오류 = 기존 fixture 갱신 (컴파일러 가이드)**

`cargo build -p automation-core` / `cargo test -p automation-core` 하면 GrantRole/CreateChannel/CreateRole/PlannedAction 구성 site가 전부 컴파일 에러로 뜬다. 각각 갱신:
- `ActionSpec::GrantRole { role: <ResourceKey expr>, target }` → `role: automation_state::RoleRef::Existing(<expr>)`(또는 import 후 `RoleRef::Existing`).
- `ActionSpec::CreateChannel { name }` → `{ key: "<적당한>".to_string(), name }`; CreateRole 동일.
- `PlannedAction::GrantRole { role: RoleId(n), target }` → `role: PlannedRole::Resolved(RoleId(n))`(assert/구성 both).
- `PlannedAction::CreateChannel { name }` → `{ key, name }`; CreateRole 동일.
대상: tests/{interpret,run,validate,modal,create,policy}.rs + rule.rs 이미 Task1. **의미 변화 없이** 래핑만.

- [ ] **Step 8: 커밋**

```bash
git add crates/automation-core
git commit -m "feat(automation-core): typed RoleRef linking, RuntimeBindings, order validate"
```

---

## Task 3: tests/linking.rs + 게이트 + push

- [ ] **Step 1: `crates/automation-core/tests/linking.rs` 작성**

```rust
use std::collections::BTreeMap;

use automation_core::adapter::{AdapterError, AdapterErrorKind, DiscordMutationAdapter};
use automation_core::event::{EventKind, RuntimeContext, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{ActionPlan, CreatedResource, PlannedAction, PlannedRole};
use automation_core::policy::{analyze, PolicyFinding};
use automation_core::run::run;
use automation_core::validate::{validate, ValidationError};
use automation_state::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, ModalFieldSpec, ModalFieldStyle,
    ModalSpec, RoleRef, TriggerSpec,
};
use discord_model::{ChannelId, GuildId, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

fn modal() -> ModalSpec {
    ModalSpec {
        key: "m".to_string(),
        title: "M".to_string(),
        fields: vec![ModalFieldSpec {
            key: "room_name".to_string(),
            label: "R".to_string(),
            style: ModalFieldStyle::Short,
            required: true,
        }],
    }
}

fn submit_rule(actions: Vec<ActionSpec>) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![modal()],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::ModalSubmit {
                modal: "m".to_string(),
            },
            actions,
        }],
    }
}

fn study_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        ActionSpec::GrantRole {
            role: RoleRef::Created {
                created: "member".to_string(),
            },
            target: ActionTarget::Actor,
        },
    ]
}

fn submit(room: &str) -> RuntimeEvent {
    let mut inputs = BTreeMap::new();
    inputs.insert("room_name".to_string(), room.to_string());
    RuntimeEvent {
        guild_id: GuildId(7),
        actor: UserId(3),
        kind: EventKind::ModalSubmit {
            modal: "m".to_string(),
            inputs,
        },
    }
}

fn plan(steps: Vec<PlannedAction>) -> ActionPlan {
    ActionPlan { steps }
}

#[test]
fn created_role_granted_to_actor() {
    let context = RuntimeContext::from_event(&submit("코딩"));
    let steps = vec![
        PlannedAction::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("member".to_string()),
            target: UserId(3),
        },
    ];
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    block_on(run(&context, &plan(steps), &mutation, &responder)).unwrap();
    assert_eq!(
        mutation.calls(),
        vec![
            MutationCall::CreateRole {
                guild: GuildId(7),
                name: "코딩 멤버".to_string(),
            },
            MutationCall::GrantRole {
                guild: GuildId(7),
                member: UserId(3),
                role: RoleId(800_000),
            },
        ]
    );
}

#[test]
fn full_study_run_creates_then_grants() {
    let context = RuntimeContext::from_event(&submit("cozy"));
    let steps = vec![
        PlannedAction::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::CreateChannel {
            key: "channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("member".to_string()),
            target: UserId(3),
        },
    ];
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let created = block_on(run(&context, &plan(steps), &mutation, &responder)).unwrap();
    assert_eq!(
        created,
        vec![
            CreatedResource::Role {
                action_index: 0,
                name: "cozy 멤버".to_string(),
                id: RoleId(800_000),
            },
            CreatedResource::Channel {
                action_index: 1,
                name: "study-cozy".to_string(),
                id: ChannelId(800_001),
            },
        ]
    );
    assert!(matches!(
        mutation.calls().as_slice(),
        [
            MutationCall::CreateRole { .. },
            MutationCall::CreateChannel { .. },
            MutationCall::GrantRole {
                role: RoleId(800_000),
                ..
            }
        ]
    ));
}

#[test]
fn duplicate_action_key_fails_validate() {
    let set = submit_rule(vec![
        ActionSpec::CreateRole {
            key: "dup".to_string(),
            name: "a".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "dup".to_string(),
            name: "b".to_string(),
        },
    ]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::DuplicateActionKey {
        rule: "r".to_string(),
        key: "dup".to_string(),
    }));
}

#[test]
fn unknown_created_role_ref_fails_validate() {
    let set = submit_rule(vec![ActionSpec::GrantRole {
        role: RoleRef::Created {
            created: "ghost".to_string(),
        },
        target: ActionTarget::Actor,
    }]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownCreatedRoleRef {
        rule: "r".to_string(),
        key: "ghost".to_string(),
    }));
}

#[test]
fn created_role_ref_to_channel_fails_validate() {
    let set = submit_rule(vec![
        ActionSpec::CreateChannel {
            key: "channel".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::GrantRole {
            role: RoleRef::Created {
                created: "channel".to_string(),
            },
            target: ActionTarget::Actor,
        },
    ]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::CreatedRoleRefTypeMismatch {
        rule: "r".to_string(),
        key: "channel".to_string(),
    }));
}

#[test]
fn forward_created_ref_fails_validate() {
    let set = submit_rule(vec![
        ActionSpec::GrantRole {
            role: RoleRef::Created {
                created: "member".to_string(),
            },
            target: ActionTarget::Actor,
        },
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
    ]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownCreatedRoleRef {
        rule: "r".to_string(),
        key: "member".to_string(),
    }));
}

#[test]
fn valid_study_ruleset_passes() {
    assert!(validate(&submit_rule(study_actions()), &ResourceBindingMap::default()).is_ok());
}

#[test]
fn create_role_failure_skips_grant() {
    struct FailCreate;
    impl DiscordMutationAdapter for FailCreate {
        async fn grant_role(
            &self,
            _g: GuildId,
            _m: UserId,
            _r: RoleId,
        ) -> Result<(), AdapterError> {
            panic!("grant_role must not run")
        }
        async fn create_role(
            &self,
            _g: GuildId,
            _s: automation_core::adapter::CreateRoleSpec,
        ) -> Result<RoleId, AdapterError> {
            Err(AdapterError::new(AdapterErrorKind::Forbidden, "no"))
        }
    }
    let context = RuntimeContext::from_event(&submit("x"));
    let steps = vec![
        PlannedAction::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("member".to_string()),
            target: UserId(3),
        },
    ];
    let responder = MockInteractionResponder::new();
    let result = block_on(run(&context, &plan(steps), &FailCreate, &responder));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
}

#[test]
fn created_template_variable_is_unsupported() {
    let set = submit_rule(vec![ActionSpec::CreateRole {
        key: "member".to_string(),
        name: "${created.member.id}".to_string(),
    }]);
    let errors = validate(&set, &ResourceBindingMap::default()).unwrap_err();
    assert!(errors.contains(&ValidationError::BadTemplate {
        rule: "r".to_string(),
    }));
}

#[test]
fn created_reference_flagged_by_policy() {
    let findings = analyze(&submit_rule(study_actions()), &BTreeMap::new());
    assert!(findings.contains(&PolicyFinding::CreatedResourceReference {
        rule: "r".to_string(),
    }));
}
```

- [ ] **Step 2~5: 게이트 + push**

- `cargo build` (경고 0) / `cargo test` (전체 ~254, 실제값 우선) / `cargo clippy --all-targets -- -D warnings` (0) / `cargo fmt --all -- --check` / `git push origin main`.
- 커밋: `feat(automation-core): created linking tests`

---

## Self-Review (스펙 대비)
- typed RoleRef(Existing/Created), CreateRole/CreateChannel key, interpret Existing해소/Created보존, run RuntimeBindings(created_roles)로 Created해소, CreateChannel key 미소비(channel은 CreatedResource) ✅.
- validate order 추적: DuplicateActionKey/UnknownCreatedRoleRef(forward 포함)/CreatedRoleRefTypeMismatch ✅.
- policy CreatedResourceReference ✅. `${created.x.id}` → BadTemplate(파서 UnsupportedVariable) ✅.
- **automation-runtime 무수정**(grant_role/create_* seam 불변; created 해소는 run) ✅.
- **GrantRole 스키마 변경**(ResourceKey→RoleRef): untagged serde로 JSON 호환, Rust fixture는 컴파일러 가이드로 RoleRef::Existing 래핑 ✅.
- clippy: `is_some_and`, `let _ = key`(16g 미소비), 중첩 match, 주석 없음 ✅.

## Codex 핸드오프 (권장 2청크)
- **청크 A** = Task 1 + Task 2. **Task2 Step7 fixture 갱신 후 automation-core 컴파일 + automation-runtime 무수정 빌드 필수.** 커밋 2개.
- **청크 B** = Task 3. tests/linking + 게이트 + push. 커밋 1개 + push.
**automation-runtime 무수정.** 완료 보고: 테스트 수 + 전체 + clippy/fmt + push 해시 + runtime 무수정 + 이탈(fixture 갱신 규모).
