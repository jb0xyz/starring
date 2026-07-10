# Phase 16h — UpsertOverwrite + ChannelRef Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** created channel/role을 참조해 permission overwrite로 비공개 채널을 조립하는 독립 primitive action. 순수 Mock.

**Architecture:** automation-state에 `CreatedRef`(공유, deny_unknown) + RoleRef를 CreatedRef로 migrate(JSON 무변) + ChannelRef + OverwriteTargetSpec + ActionSpec::UpsertOverwrite. automation-core에 PlannedChannel/PlannedOverwriteTarget + upsert_overwrite seam(default-unsupported) + RuntimeBindings.created_channels + interpret/run 해소 + validate + policy. **automation-runtime 무수정.**

## Global Constraints
- **코드 주석 금지.** **Codex 구현.**
- **automation-runtime 무수정** — 새 seam은 default-unsupported.
- **@everyone = `OverwriteTarget::Role(RoleId(guild_id.0))`** (GuildId.0은 pub).
- allow/deny = `discord_model::Permissions`(비트 serde). 이름배열 DSL은 후속.
- 완료 게이트: build(경고0)/test/clippy(`--all-targets -- -D warnings`)/fmt. 완료 후 push.
- serde 형태 실증됨: existing=bare string, created=`{created:x}`(deny_unknown), everyone=`"everyone"`, role=`{role:<RoleRef>}`.

---

## Task 1: automation-state — CreatedRef + ChannelRef + OverwriteTargetSpec + UpsertOverwrite

- [ ] **Step 1: `Cargo.toml` — discord-model 의존 추가**

`[dependencies]`에 추가(desired-state 줄 다음):
```toml
discord-model = { path = "../discord-model" }
```

- [ ] **Step 2: `rule.rs` — import + 타입**

파일 상단 import에 추가:
```rust
use discord_model::Permissions;
```

`ActionSpec` enum에 UpsertOverwrite 변형 추가(CreateRole 다음):
```rust
    UpsertOverwrite {
        channel: ChannelRef,
        target: OverwriteTargetSpec,
        #[serde(default)]
        allow: Permissions,
        #[serde(default)]
        deny: Permissions,
    },
```

기존 `RoleRef` 정의를 다음으로 **교체**(CreatedRef 도입 + ChannelRef/OverwriteTargetSpec 추가):
```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatedRef {
    pub created: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoleRef {
    Existing(ResourceKey),
    Created(CreatedRef),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChannelRef {
    Existing(ResourceKey),
    Created(CreatedRef),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverwriteTargetSpec {
    Everyone,
    Role(RoleRef),
}
```

- [ ] **Step 3: `lib.rs` — 재노출**

`pub use rule::{...}` 를:
```rust
pub use rule::{
    ActionSpec, ActionTarget, ChannelRef, CreatedRef, InteractionRule, InteractionRuleSet,
    OverwriteTargetSpec, RoleRef, TriggerSpec,
};
```

- [ ] **Step 4: `rule.rs` 테스트 — serde 고정(S1~S4)**

test 모듈 상단(`use desired_state::ResourceKey;` 옆)에 추가:
```rust
    use discord_model::Permissions;
```
test 모듈 끝(닫는 `}` 앞)에 추가:
```rust
    #[test]
    fn channel_ref_serde_shapes() {
        assert_eq!(
            serde_json::from_str::<ChannelRef>(r#""general""#).unwrap(),
            ChannelRef::Existing(ResourceKey("general".to_string()))
        );
        let created = ChannelRef::Created(CreatedRef {
            created: "study_channel".to_string(),
        });
        assert_eq!(
            serde_json::to_string(&created).unwrap(),
            r#"{"created":"study_channel"}"#
        );
        assert_eq!(
            serde_json::from_str::<ChannelRef>(r#"{"created":"study_channel"}"#).unwrap(),
            created
        );
    }

    #[test]
    fn overwrite_target_serde_shapes() {
        assert_eq!(
            serde_json::to_string(&OverwriteTargetSpec::Everyone).unwrap(),
            r#""everyone""#
        );
        assert_eq!(
            serde_json::from_str::<OverwriteTargetSpec>(r#""everyone""#).unwrap(),
            OverwriteTargetSpec::Everyone
        );
        let existing = OverwriteTargetSpec::Role(RoleRef::Existing(ResourceKey(
            "verified_member".to_string(),
        )));
        assert_eq!(
            serde_json::to_string(&existing).unwrap(),
            r#"{"role":"verified_member"}"#
        );
        let created = OverwriteTargetSpec::Role(RoleRef::Created(CreatedRef {
            created: "study_member_role".to_string(),
        }));
        assert_eq!(
            serde_json::to_string(&created).unwrap(),
            r#"{"role":{"created":"study_member_role"}}"#
        );
        assert_eq!(
            serde_json::from_str::<OverwriteTargetSpec>(r#"{"role":{"created":"study_member_role"}}"#)
                .unwrap(),
            created
        );
    }

    #[test]
    fn created_ref_rejects_unknown_field() {
        assert!(serde_json::from_str::<ChannelRef>(r#"{"created":"x","extra":"y"}"#).is_err());
        assert!(serde_json::from_str::<RoleRef>(r#"{"created":"x","extra":"y"}"#).is_err());
    }

    #[test]
    fn upsert_overwrite_action_roundtrips() {
        let json = r#"{"type":"upsert_overwrite","channel":{"created":"study_channel"},"target":"everyone","allow":"0","deny":"1024"}"#;
        let action: ActionSpec = serde_json::from_str(json).unwrap();
        assert_eq!(
            action,
            ActionSpec::UpsertOverwrite {
                channel: ChannelRef::Created(CreatedRef {
                    created: "study_channel".to_string(),
                }),
                target: OverwriteTargetSpec::Everyone,
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            }
        );
        let unknown =
            r#"{"type":"upsert_overwrite","channel":"g","target":"everyone","evil":1}"#;
        assert!(serde_json::from_str::<ActionSpec>(unknown).is_err());
    }
```

- [ ] **Step 5: 테스트 + 커밋**

Run: `cargo test -p automation-state` (기존 + S1~S4 통과)
```bash
git add crates/automation-state
git commit -m "feat(automation-state): CreatedRef, ChannelRef, OverwriteTargetSpec, UpsertOverwrite"
```

---

## Task 2: automation-core — PlannedChannel · seam · interpret · run · validate · policy

- [ ] **Step 1: `plan.rs` — import + 타입 추가**

import 교체: `use discord_model::{ChannelId, Permissions, RoleId, UserId};`

`PlannedRole` enum 다음에 추가:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedChannel {
    Resolved(ChannelId),
    Created(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedOverwriteTarget {
    Everyone,
    Role(PlannedRole),
}
```

`PlannedAction` enum에 변형 추가(CreateRole 다음):
```rust
    UpsertOverwrite {
        channel: PlannedChannel,
        target: PlannedOverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
```

- [ ] **Step 2: `adapter.rs` — upsert_overwrite seam (default-unsupported)**

import 교체: `use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId, UserId};`

`DiscordMutationAdapter` trait의 `create_role` 다음에 추가:
```rust
    async fn upsert_overwrite(
        &self,
        _guild: GuildId,
        _channel: ChannelId,
        _target: OverwriteTarget,
        _allow: Permissions,
        _deny: Permissions,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "upsert_overwrite is not supported",
        ))
    }
```

- [ ] **Step 3: `mock.rs` — MutationCall + impl**

import 교체: `use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId, UserId};`

`MutationCall` enum에 변형 추가(CreateRole 다음):
```rust
    UpsertOverwrite {
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
```

`impl DiscordMutationAdapter for MockMutationAdapter`의 `create_role` 다음에 추가:
```rust
    async fn upsert_overwrite(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(MutationCall::UpsertOverwrite {
                guild,
                channel,
                target,
                allow,
                deny,
            });
        match &self.fail {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
```

- [ ] **Step 4: `interpret.rs` 전체 교체** (RoleRef migrate + resolve_role 헬퍼 + UpsertOverwrite)

```rust
use automation_state::{
    ActionSpec, ActionTarget, ChannelRef, InteractionRuleSet, OverwriteTargetSpec, RoleRef,
    TriggerSpec,
};
use resource_resolution::ResourceBindingMap;

use crate::event::{EventKind, RuntimeEvent};
use crate::plan::{
    ActionPlan, ModalPresentation, PlannedAction, PlannedChannel, PlannedOverwriteTarget,
    PlannedRole,
};

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
                let planned_role = resolve_role(role, bindings)?;
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
            ActionSpec::UpsertOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                let planned_channel = match channel {
                    ChannelRef::Existing(key) => {
                        PlannedChannel::Resolved(*bindings.channel_bindings.get(key)?)
                    }
                    ChannelRef::Created(inner) => PlannedChannel::Created(inner.created.clone()),
                };
                let planned_target = match target {
                    OverwriteTargetSpec::Everyone => PlannedOverwriteTarget::Everyone,
                    OverwriteTargetSpec::Role(role) => {
                        PlannedOverwriteTarget::Role(resolve_role(role, bindings)?)
                    }
                };
                steps.push(PlannedAction::UpsertOverwrite {
                    channel: planned_channel,
                    target: planned_target,
                    allow: *allow,
                    deny: *deny,
                });
            }
        }
    }

    Some(ActionPlan { steps })
}

fn resolve_role(role: &RoleRef, bindings: &ResourceBindingMap) -> Option<PlannedRole> {
    match role {
        RoleRef::Existing(key) => Some(PlannedRole::Resolved(*bindings.role_bindings.get(key)?)),
        RoleRef::Created(inner) => Some(PlannedRole::Created(inner.created.clone())),
    }
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

- [ ] **Step 5: `run.rs` 전체 교체** (created_channels + UpsertOverwrite 해소)

```rust
use std::collections::BTreeMap;

use automation_state::InteractionRuleSet;
use discord_model::{ChannelId, OverwriteTarget, RoleId};
use resource_resolution::ResourceBindingMap;

use crate::adapter::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    InteractionResponder,
};
use crate::event::{RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{
    ActionPlan, CreatedResource, PlannedAction, PlannedChannel, PlannedOverwriteTarget, PlannedRole,
};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

#[derive(Default)]
struct RuntimeBindings {
    created_roles: BTreeMap<String, RoleId>,
    created_channels: BTreeMap<String, ChannelId>,
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
                let role_id = resolve_planned_role(role, &runtime)?;
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
            PlannedAction::CreateChannel { key, name } => {
                let rendered = render(name, context, SanitizeContext::ChannelName)?;
                let id = mutation
                    .create_channel(
                        context.guild_id,
                        CreateChannelSpec {
                            name: rendered.clone(),
                        },
                    )
                    .await?;
                runtime.created_channels.insert(key.clone(), id);
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
            PlannedAction::UpsertOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                let channel_id = match channel {
                    PlannedChannel::Resolved(id) => *id,
                    PlannedChannel::Created(key) => *runtime
                        .created_channels
                        .get(key)
                        .ok_or_else(|| unresolved_created_channel(key))?,
                };
                let overwrite_target = match target {
                    PlannedOverwriteTarget::Everyone => {
                        OverwriteTarget::Role(RoleId(context.guild_id.0))
                    }
                    PlannedOverwriteTarget::Role(role) => {
                        OverwriteTarget::Role(resolve_planned_role(role, &runtime)?)
                    }
                };
                mutation
                    .upsert_overwrite(context.guild_id, channel_id, overwrite_target, *allow, *deny)
                    .await?;
            }
        }
    }
    Ok(created)
}

fn resolve_planned_role(
    role: &PlannedRole,
    runtime: &RuntimeBindings,
) -> Result<RoleId, AdapterError> {
    match role {
        PlannedRole::Resolved(id) => Ok(*id),
        PlannedRole::Created(key) => runtime
            .created_roles
            .get(key)
            .copied()
            .ok_or_else(|| unresolved_created_role(key)),
    }
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

fn unresolved_created_channel(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved created channel: {key}"),
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
> `resolve_planned_role` 헬퍼로 GrantRole + UpsertOverwrite target의 Created role 해소를 DRY. 16g의 inline `.ok_or_else` 로직과 동일.

- [ ] **Step 6: `validate.rs` — import + 5 error + 액션 루프 + 헬퍼**

import 교체:
```rust
use automation_state::{
    ActionSpec, ChannelRef, InteractionRule, InteractionRuleSet, OverwriteTargetSpec, RoleRef,
    TriggerSpec,
};
```

`ValidationError` enum의 `CreatedRoleRefTypeMismatch {...}` 다음에 추가:
```rust
    UnknownChannelRef {
        rule: String,
        channel: ResourceKey,
    },
    UnknownCreatedChannelRef {
        rule: String,
        key: String,
    },
    CreatedChannelRefTypeMismatch {
        rule: String,
        key: String,
    },
    OverlappingOverwrite {
        rule: String,
    },
    EmptyOverwrite {
        rule: String,
    },
```

`for action in &rule.actions { match action { ... } }` 블록을 다음으로 **교체**:
```rust
        for action in &rule.actions {
            match action {
                ActionSpec::GrantRole { role, .. } => {
                    check_role_ref(&mut errors, rule, bindings, &created, role);
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
                ActionSpec::UpsertOverwrite {
                    channel,
                    target,
                    allow,
                    deny,
                } => {
                    check_channel_ref(&mut errors, rule, bindings, &created, channel);
                    if let OverwriteTargetSpec::Role(role) = target {
                        check_role_ref(&mut errors, rule, bindings, &created, role);
                    }
                    if allow.intersects(*deny) {
                        errors.push(ValidationError::OverlappingOverwrite {
                            rule: rule.key.clone(),
                        });
                    }
                    if allow.is_empty() && deny.is_empty() {
                        errors.push(ValidationError::EmptyOverwrite {
                            rule: rule.key.clone(),
                        });
                    }
                }
            }
        }
```

`check_template` 함수 다음에 헬퍼 추가:
```rust
fn check_role_ref(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    bindings: &ResourceBindingMap,
    created: &BTreeMap<String, CreatedKind>,
    role: &RoleRef,
) {
    match role {
        RoleRef::Existing(key) => {
            if !bindings.role_bindings.contains_key(key) {
                errors.push(ValidationError::UnknownRoleRef {
                    rule: rule.key.clone(),
                    role: key.clone(),
                });
            }
        }
        RoleRef::Created(inner) => match created.get(&inner.created) {
            None => errors.push(ValidationError::UnknownCreatedRoleRef {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
            Some(CreatedKind::Channel) => {
                errors.push(ValidationError::CreatedRoleRefTypeMismatch {
                    rule: rule.key.clone(),
                    key: inner.created.clone(),
                })
            }
            Some(CreatedKind::Role) => {}
        },
    }
}

fn check_channel_ref(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    bindings: &ResourceBindingMap,
    created: &BTreeMap<String, CreatedKind>,
    channel: &ChannelRef,
) {
    match channel {
        ChannelRef::Existing(key) => {
            if !bindings.channel_bindings.contains_key(key) {
                errors.push(ValidationError::UnknownChannelRef {
                    rule: rule.key.clone(),
                    channel: key.clone(),
                });
            }
        }
        ChannelRef::Created(inner) => match created.get(&inner.created) {
            None => errors.push(ValidationError::UnknownCreatedChannelRef {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
            Some(CreatedKind::Role) => {
                errors.push(ValidationError::CreatedChannelRefTypeMismatch {
                    rule: rule.key.clone(),
                    key: inner.created.clone(),
                })
            }
            Some(CreatedKind::Channel) => {}
        },
    }
}
```

- [ ] **Step 7: `policy.rs` 전체 교체** (RoleRef migrate + UpsertOverwrite)

```rust
use std::collections::BTreeMap;

use automation_state::{ActionSpec, InteractionRuleSet, OverwriteTargetSpec, RoleRef};
use desired_state::ResourceKey;
use discord_model::Permissions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyFinding {
    PrivilegedRoleGrant { rule: String, role: ResourceKey },
    DynamicResourceCreation { rule: String, action: DynamicAction },
    CreatedResourceReference { rule: String },
    EveryoneOverwrite { rule: String },
    PrivilegedOverwriteAllow { rule: String },
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
                ActionSpec::GrantRole { role, .. } => match role {
                    RoleRef::Existing(key) => {
                        if roles.get(key).is_some_and(|perms| perms.intersects(mask)) {
                            findings.push(PolicyFinding::PrivilegedRoleGrant {
                                rule: rule.key.clone(),
                                role: key.clone(),
                            });
                        }
                    }
                    RoleRef::Created(..) => {
                        findings.push(PolicyFinding::CreatedResourceReference {
                            rule: rule.key.clone(),
                        });
                    }
                },
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
                ActionSpec::UpsertOverwrite { target, allow, .. } => {
                    if matches!(target, OverwriteTargetSpec::Everyone) {
                        findings.push(PolicyFinding::EveryoneOverwrite {
                            rule: rule.key.clone(),
                        });
                    }
                    if allow.intersects(mask) {
                        findings.push(PolicyFinding::PrivilegedOverwriteAllow {
                            rule: rule.key.clone(),
                        });
                    }
                }
                ActionSpec::RespondEphemeral { .. } | ActionSpec::OpenModal { .. } => {}
            }
        }
    }
    findings
}
```

- [ ] **Step 8: `lib.rs` — 재노출**

`pub use plan::{...}` 를:
```rust
pub use plan::{
    ActionPlan, CreatedResource, ModalPresentation, PlannedAction, PlannedChannel,
    PlannedOverwriteTarget, PlannedRole,
};
```

- [ ] **Step 9: 빌드 + 커밋**

Run: `cargo build -p automation-core` (경고 0) / `cargo build -p automation-runtime` (무수정 성공)
> 이 시점 automation-core 테스트는 아직 안 고침(linking.rs의 RoleRef::Created가 Task 3에서 migrate) — build만 확인.
```bash
git add crates/automation-core
git commit -m "feat(automation-core): UpsertOverwrite planning, seam, run, validate, policy"
```

---

## Task 3: tests/linking migrate + tests/overwrite.rs + 게이트 + push

- [ ] **Step 1: `tests/linking.rs` — RoleRef::Created migrate (4곳)**

import에 CreatedRef 추가(`use automation_state::{...};` 안):
```rust
    ChannelRef, CreatedRef,
```
> 실제로는 `RoleRef` 옆에 `CreatedRef` 만 추가하면 됨(ChannelRef는 linking.rs에서 불필요하면 생략).

4곳의 `RoleRef::Created { created: X }` → `RoleRef::Created(CreatedRef { created: X })`. 컴파일러가 각 site를 잡음(라인 57/184/204/223 부근). 의미 변화 없음.

- [ ] **Step 2: `crates/automation-core/tests/overwrite.rs` 신설**

```rust
use std::collections::BTreeMap;

use automation_core::event::{EventKind, RuntimeContext, RuntimeEvent};
use automation_core::mock::{MockInteractionResponder, MockMutationAdapter, MutationCall};
use automation_core::plan::{
    ActionPlan, PlannedAction, PlannedChannel, PlannedOverwriteTarget, PlannedRole,
};
use automation_core::policy::{analyze, PolicyFinding};
use automation_core::run::run;
use automation_core::validate::{validate, ValidationError};
use automation_state::{
    ActionSpec, ActionTarget, ChannelRef, CreatedRef, InteractionRule, InteractionRuleSet,
    ModalFieldSpec, ModalFieldStyle, ModalSpec, OverwriteTargetSpec, RoleRef, TriggerSpec,
};
use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId, UserId};
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

fn overwrite_rule(actions: Vec<ActionSpec>) -> InteractionRuleSet {
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

fn channel_created(key: &str) -> ChannelRef {
    ChannelRef::Created(CreatedRef {
        created: key.to_string(),
    })
}

fn role_created(key: &str) -> RoleRef {
    RoleRef::Created(CreatedRef {
        created: key.to_string(),
    })
}

fn private_study_actions() -> Vec<ActionSpec> {
    vec![
        ActionSpec::CreateRole {
            key: "study_member_role".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        ActionSpec::CreateChannel {
            key: "study_channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("study_channel"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("study_channel"),
            target: OverwriteTargetSpec::Role(role_created("study_member_role")),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
        ActionSpec::GrantRole {
            role: role_created("study_member_role"),
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

fn run_plan(steps: Vec<PlannedAction>) -> Vec<MutationCall> {
    let context = RuntimeContext::from_event(&submit("cozy"));
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    block_on(run(&context, &ActionPlan { steps }, &mutation, &responder)).unwrap();
    mutation.calls()
}

#[test]
fn everyone_overwrite_on_created_channel_resolves() {
    let calls = run_plan(vec![
        PlannedAction::CreateChannel {
            key: "c".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("c".to_string()),
            target: PlannedOverwriteTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
    ]);
    assert_eq!(
        calls,
        vec![
            MutationCall::CreateChannel {
                guild: GuildId(7),
                name: "study-cozy".to_string(),
            },
            MutationCall::UpsertOverwrite {
                guild: GuildId(7),
                channel: ChannelId(800_000),
                target: OverwriteTarget::Role(RoleId(7)),
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            },
        ]
    );
}

#[test]
fn created_role_target_resolves() {
    let calls = run_plan(vec![
        PlannedAction::CreateRole {
            key: "r".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::CreateChannel {
            key: "c".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("c".to_string()),
            target: PlannedOverwriteTarget::Role(PlannedRole::Created("r".to_string())),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
    ]);
    assert_eq!(
        calls.last().unwrap(),
        &MutationCall::UpsertOverwrite {
            guild: GuildId(7),
            channel: ChannelId(800_001),
            target: OverwriteTarget::Role(RoleId(800_000)),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        }
    );
}

#[test]
fn private_study_room_call_sequence() {
    let calls = run_plan(vec![
        PlannedAction::CreateRole {
            key: "study_member_role".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        PlannedAction::CreateChannel {
            key: "study_channel".to_string(),
            name: "study-${input.room_name}".to_string(),
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("study_channel".to_string()),
            target: PlannedOverwriteTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
        PlannedAction::UpsertOverwrite {
            channel: PlannedChannel::Created("study_channel".to_string()),
            target: PlannedOverwriteTarget::Role(PlannedRole::Created(
                "study_member_role".to_string(),
            )),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
        PlannedAction::GrantRole {
            role: PlannedRole::Created("study_member_role".to_string()),
            target: UserId(3),
        },
    ]);
    assert_eq!(
        calls,
        vec![
            MutationCall::CreateRole {
                guild: GuildId(7),
                name: "cozy 멤버".to_string(),
            },
            MutationCall::CreateChannel {
                guild: GuildId(7),
                name: "study-cozy".to_string(),
            },
            MutationCall::UpsertOverwrite {
                guild: GuildId(7),
                channel: ChannelId(800_001),
                target: OverwriteTarget::Role(RoleId(7)),
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            },
            MutationCall::UpsertOverwrite {
                guild: GuildId(7),
                channel: ChannelId(800_001),
                target: OverwriteTarget::Role(RoleId(800_000)),
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
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
fn channel_ref_missing_created_key_fails() {
    let set = overwrite_rule(vec![ActionSpec::UpsertOverwrite {
        channel: channel_created("ghost"),
        target: OverwriteTargetSpec::Everyone,
        allow: Permissions::empty(),
        deny: Permissions::VIEW_CHANNEL,
    }]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::UnknownCreatedChannelRef {
            rule: "r".to_string(),
            key: "ghost".to_string(),
        }));
}

#[test]
fn channel_ref_to_role_key_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateRole {
            key: "somerole".to_string(),
            name: "x".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("somerole"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::CreatedChannelRefTypeMismatch {
            rule: "r".to_string(),
            key: "somerole".to_string(),
        }));
}

#[test]
fn role_target_missing_created_key_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Role(role_created("ghost")),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::UnknownCreatedRoleRef {
            rule: "r".to_string(),
            key: "ghost".to_string(),
        }));
}

#[test]
fn role_target_to_channel_key_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Role(role_created("c")),
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::CreatedRoleRefTypeMismatch {
            rule: "r".to_string(),
            key: "c".to_string(),
        }));
}

#[test]
fn forward_channel_ref_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        },
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::UnknownCreatedChannelRef {
            rule: "r".to_string(),
            key: "c".to_string(),
        }));
}

#[test]
fn allow_deny_overlap_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::VIEW_CHANNEL,
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::OverlappingOverwrite {
            rule: "r".to_string(),
        }));
}

#[test]
fn allow_deny_both_empty_fails() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::empty(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EmptyOverwrite {
            rule: "r".to_string(),
        }));
}

#[test]
fn valid_private_study_passes() {
    assert!(validate(&overwrite_rule(private_study_actions()), &ResourceBindingMap::default()).is_ok());
}

#[test]
fn everyone_overwrite_flagged_by_policy() {
    let findings = analyze(&overwrite_rule(private_study_actions()), &BTreeMap::new());
    assert!(findings.contains(&PolicyFinding::EveryoneOverwrite {
        rule: "r".to_string(),
    }));
}

#[test]
fn privileged_allow_flagged_by_policy() {
    let set = overwrite_rule(vec![
        ActionSpec::CreateChannel {
            key: "c".to_string(),
            name: "study".to_string(),
        },
        ActionSpec::UpsertOverwrite {
            channel: channel_created("c"),
            target: OverwriteTargetSpec::Everyone,
            allow: Permissions::ADMINISTRATOR,
            deny: Permissions::empty(),
        },
    ]);
    let findings = analyze(&set, &BTreeMap::new());
    assert!(findings.contains(&PolicyFinding::PrivilegedOverwriteAllow {
        rule: "r".to_string(),
    }));
}
```

- [ ] **Step 3~6: 게이트 + push**
- `cargo build` (경고 0) / `cargo test` (전체 ~270, 실제값 우선) / `cargo clippy --all-targets -- -D warnings` (0) / `cargo fmt --all -- --check` / `git push origin main`.
- 커밋: `feat(automation-core): private channel overwrite tests`

---

## Self-Review (스펙 대비)
- UpsertOverwrite 독립 action, ChannelRef{Existing,Created}, OverwriteTargetSpec{Everyone, Role(RoleRef)}, allow/deny Permissions ✅.
- interpret Existing해소(channel_bindings/role_bindings)/Created보존, run이 created_channels로 ChannelRef::Created 해소·@everyone=RoleId(guild.0) ✅.
- validate: channel/role ref(check_role_ref/check_channel_ref 헬퍼) + forward/type + **overlap(intersects)/empty(is_empty)** ✅.
- policy EveryoneOverwrite + PrivilegedOverwriteAllow ✅.
- **CreatedRef(deny_unknown) 공유** — `{created:x,extra:y}` REJECT(원칙 #2), RoleRef migrate JSON 무변 ✅. serde 고정 S1~S4 ✅.
- seam upsert_overwrite default-unsupported → **automation-runtime 무수정** ✅.
- 주석 없음, clippy(is_some_and/matches!/intersects) ✅.

## Codex 핸드오프 (권장 3청크)
- **청크 A** = Task 1(automation-state). serde 고정 포함. 커밋 1개.
- **청크 B** = Task 2(automation-core 로직). **build만**(linking.rs는 C에서 migrate). automation-runtime 무수정 빌드 필수. 커밋 1개.
- **청크 C** = Task 3(linking migrate + overwrite tests + 게이트 + push). 커밋 1개 + push.
**automation-runtime 무수정.** 보고: 테스트 수 + 전체 + clippy/fmt + push 해시 + runtime 무수정 + 이탈.
