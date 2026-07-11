# Phase 17b — Instance Registration Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`).

**Goal:** `RegisterInstance{key, kind, resources}`가 현재 run의 created role/channel/message를 명시적 manifest로 선택→guild-scoped `AutomationInstance`로 store 등록→InstanceId를 action key에 바인딩.

**Architecture:** automation-instance에 IdGenerator. automation-state에 RegisterInstance + InstanceResourceRefs + PostPanel key. automation-core: CreatedResource key+Instance, created_messages, RuntimeContext ruleset_key, RegisterInstance interpret/run/validate, run/handle_event에 instances/instance_ids 배선. automation-runtime/tool 배선.

## Global Constraints
- **코드 주석 금지.** **Codex 구현.**
- **created-only** manifest(CreatedRef). metadata(guild/ruleset_key/actor/status)는 **시스템**(context)이 채움 — action은 key/kind/resource-refs만.
- **실행 순서 불변식**: refs 해소→id 생성→register→**성공 후에만** binding. 실패=fail-fast(`?`).
- **output key 단일 namespace**(CreateRole/CreateChannel/PostPanel/RegisterInstance) — 중복 금지. button key/rule key는 별도.
- **automation-instance는 순수 유지**(Postgres는 17d 별도 crate). 게이트 build/test/clippy(-D warnings)/fmt. push. **live 없음.**

---

## Task A: automation-instance IdGenerator + automation-state RegisterInstance/PostPanel key

- [ ] **Step 1: `automation-instance/src/generator.rs`(신규) + lib 재노출**

```rust
use std::sync::atomic::{AtomicU64, Ordering};

use crate::id::{InstanceId, InstanceIdError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceIdGenerationError {
    Invalid(InstanceIdError),
}

pub trait InstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError>;
}

pub struct SequenceInstanceIdGenerator {
    prefix: String,
    next: AtomicU64,
}

impl SequenceInstanceIdGenerator {
    pub fn new(prefix: &str, start: u64) -> Self {
        Self {
            prefix: prefix.to_string(),
            next: AtomicU64::new(start),
        }
    }
}

impl InstanceIdGenerator for SequenceInstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError> {
        let value = self.next.fetch_add(1, Ordering::SeqCst);
        InstanceId::parse(&format!("{}_{:03}", self.prefix, value))
            .map_err(InstanceIdGenerationError::Invalid)
    }
}
```
lib.rs에 `pub mod generator;` + `pub use generator::{InstanceIdGenerationError, InstanceIdGenerator, SequenceInstanceIdGenerator};`

Run: `cargo test -p automation-instance` → 커밋 `feat(automation-instance): instance id generator`

- [ ] **Step 2: `automation-state/Cargo.toml` — automation-instance dep**

`[dependencies]`에: `automation-instance = { path = "../automation-instance" }`

- [ ] **Step 3: `automation-state/rule.rs` — RegisterInstance + PostPanel key + InstanceResourceRefs**

import: `use automation_instance::InstanceKind;` + `use crate::rule::CreatedRef;`(이미 rule.rs 내 정의면 생략) + `use std::collections::BTreeMap;`

`ActionSpec`의 PostPanel에 `key` 추가 + EditResponse 다음 RegisterInstance 추가:
```rust
    PostPanel {
        key: String,
        channel: ChannelRef,
        content: String,
        #[serde(default)]
        buttons: Vec<ButtonSpec>,
    },
    ...
    RegisterInstance {
        key: String,
        kind: InstanceKind,
        resources: InstanceResourceRefs,
    },
```

파일에 InstanceResourceRefs 추가:
```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceResourceRefs {
    #[serde(default)]
    pub roles: BTreeMap<String, CreatedRef>,
    #[serde(default)]
    pub channels: BTreeMap<String, CreatedRef>,
    #[serde(default)]
    pub messages: BTreeMap<String, CreatedRef>,
}
```
lib.rs에 **`pub use automation_instance::InstanceKind;` + `InstanceResourceRefs` 재노출**. 그러면 automation-state/automation-core 모두 `automation_state::InstanceKind`로 일관 사용(또는 각자 automation_instance에서 직접 import — 컴파일러 가이드).

> `InstanceKind`는 `automation-instance`가 소유. automation-state가 dep으로 사용·재노출(순환 없음: automation-instance는 discord-model/serde만).

- [ ] **Step 4: rule.rs 테스트 fixture 갱신(컴파일러 가이드) + serde 테스트**

기존 PostPanel 구성(rule.rs 테스트)에 `key` 추가. RegisterInstance serde roundtrip 테스트 추가:
```rust
    #[test]
    fn register_instance_roundtrip() {
        let json = r#"{"type":"register_instance","key":"study_room_instance","kind":"study_room","resources":{"roles":{"member_role":{"created":"study_member_role"}},"channels":{},"messages":{}}}"#;
        let action: ActionSpec = serde_json::from_str(json).unwrap();
        match action {
            ActionSpec::RegisterInstance { key, kind, resources } => {
                assert_eq!(key, "study_room_instance");
                assert_eq!(kind, InstanceKind("study_room".to_string()));
                assert_eq!(
                    resources.roles.get("member_role").unwrap().created,
                    "study_member_role"
                );
            }
            _ => panic!("wrong variant"),
        }
    }
```

Run: `cargo test -p automation-state` → 커밋 `feat(automation-state): RegisterInstance action + PostPanel key`

---

## Task B: automation-core types + bindings

- [ ] **Step 1: `automation-core/Cargo.toml` — automation-instance dep**

`[dependencies]`에: `automation-instance = { path = "../automation-instance" }`

- [ ] **Step 2: `plan.rs` — CreatedResource key + Instance, PlannedAction PostPanel key + RegisterInstance**

import: `use automation_instance::InstanceId;` + `use automation_state::{ButtonSpec, InstanceResourceRefs, InstanceKind, ModalFieldSpec};`(InstanceResourceRefs/InstanceKind 추가) + MessageId(이미).

`CreatedResource` 교체:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreatedResource {
    Channel { action_index: usize, key: String, name: String, id: ChannelId },
    Role { action_index: usize, key: String, name: String, id: RoleId },
    Message { action_index: usize, key: String, channel: ChannelId, id: MessageId },
    Instance { action_index: usize, key: String, id: InstanceId },
}
```

`PlannedAction::PostPanel`에 key 추가 + EditResponse 다음 RegisterInstance:
```rust
    PostPanel {
        key: String,
        channel: PlannedChannel,
        content: String,
        buttons: Vec<ButtonSpec>,
    },
    ...
    RegisterInstance {
        key: String,
        kind: InstanceKind,
        resources: InstanceResourceRefs,
    },
```
> PlannedAction::RegisterInstance는 ActionSpec의 resources(CreatedRef maps)를 그대로 운반(created-only, run이 해소).

- [ ] **Step 3: `event.rs` — RuntimeContext에 ruleset_key**

`RuntimeContext`에 `pub ruleset_key: String` 추가. `from_event`를 `from_event(event: &RuntimeEvent, ruleset_key: &str)`로 변경:
```rust
    pub fn from_event(event: &RuntimeEvent, ruleset_key: &str) -> Self {
        let inputs = match &event.kind {
            EventKind::ModalSubmit { inputs, .. } => inputs.clone(),
            EventKind::ButtonClick { .. } => BTreeMap::new(),
        };
        Self {
            guild_id: event.guild_id,
            actor: event.actor,
            ruleset_key: ruleset_key.to_string(),
            inputs,
        }
    }
```
> from_event 호출자(run.rs handle_event + 테스트)는 ruleset_key 인자 추가(컴파일러 가이드; 테스트는 `"test"` 등).

- [ ] **Step 4: 빌드(부분) + 커밋**

이 시점 automation-core는 run/validate 미갱신으로 컴파일 실패 — 정상. `cargo build -p automation-state` + `cargo build -p automation-instance` 통과 확인 후:
```bash
git add crates/automation-instance crates/automation-state crates/automation-core Cargo.toml Cargo.lock
git commit -m "feat(automation-core): registration types (CreatedResource key/Instance, RegisterInstance planned)"
```

---

## Task C: interpret/run/validate 로직 + 호출자 파급 + 테스트

- [ ] **Step 1: `interpret.rs` — PostPanel key + RegisterInstance arm**

PostPanel arm에 key 운반(ActionSpec::PostPanel{key, channel, content, buttons} → PlannedAction::PostPanel{key: key.clone(), ...}). EditResponse arm 다음(있으면) RegisterInstance arm 추가:
```rust
            ActionSpec::RegisterInstance { key, kind, resources } => {
                steps.push(PlannedAction::RegisterInstance {
                    key: key.clone(),
                    kind: kind.clone(),
                    resources: resources.clone(),
                });
            }
```
(DeferEphemeral/EditResponse arm도 이미 있으면 유지 — 16k.)

- [ ] **Step 2: `run.rs` — created_messages + CreatedResource key + RegisterInstance arm + instances/instance_ids 파라미터**

import: `use automation_instance::{AutomationInstance, InstanceId, InstanceResources, InstanceStatus, InstanceStore, InstanceIdGenerator};` + `use automation_state::{InstanceKind, InstanceResourceRefs};` + MessageId.

`RuntimeBindings`에 `created_messages: BTreeMap<String, discord_model::MessageId>` 추가.

`run` 시그니처에 파라미터 2개 추가(마지막):
```rust
pub async fn run(
    context: &RuntimeContext,
    plan: &ActionPlan,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
    instances: &impl InstanceStore,
    instance_ids: &impl InstanceIdGenerator,
) -> Result<Vec<CreatedResource>, AdapterError> {
```

CreateRole/CreateChannel arm의 `created.push`에 `key: key.clone()` 추가(CreatedResource::Role/Channel). PostPanel arm: `key` 바인딩 + `runtime.created_messages.insert(key.clone(), id)` + `created.push(CreatedResource::Message{action_index, key: key.clone(), channel: channel_id, id})`.

PlannedAction::RegisterInstance arm 추가(불변식 순서):
```rust
            PlannedAction::RegisterInstance { key, kind, resources } => {
                let resolved = resolve_manifest(resources, &runtime)?;
                let id = instance_ids
                    .generate()
                    .map_err(|error| AdapterError::new(
                        AdapterErrorKind::BadRequest,
                        format!("instance id error: {error:?}"),
                    ))?;
                let instance = AutomationInstance {
                    id: id.clone(),
                    guild_id: context.guild_id,
                    ruleset_key: context.ruleset_key.clone(),
                    kind: kind.clone(),
                    created_by: context.actor,
                    resources: resolved,
                    status: InstanceStatus::Active,
                };
                instances
                    .register(instance)
                    .await
                    .map_err(|error| AdapterError::new(
                        AdapterErrorKind::BadRequest,
                        format!("instance register error: {error:?}"),
                    ))?;
                runtime.created_instances.insert(key.clone(), id.clone());
                created.push(CreatedResource::Instance {
                    action_index,
                    key: key.clone(),
                    id,
                });
            }
```
`RuntimeBindings`에 `created_instances: BTreeMap<String, InstanceId>`도 추가.

manifest 해소 헬퍼(파일 하단):
```rust
fn resolve_manifest(
    refs: &InstanceResourceRefs,
    runtime: &RuntimeBindings,
) -> Result<InstanceResources, AdapterError> {
    let mut resources = InstanceResources::default();
    for (alias, created) in &refs.roles {
        let id = runtime
            .created_roles
            .get(&created.created)
            .copied()
            .ok_or_else(|| unresolved_manifest(&created.created))?;
        resources.roles.insert(alias.clone(), id);
    }
    for (alias, created) in &refs.channels {
        let id = runtime
            .created_channels
            .get(&created.created)
            .copied()
            .ok_or_else(|| unresolved_manifest(&created.created))?;
        resources.channels.insert(alias.clone(), id);
    }
    for (alias, created) in &refs.messages {
        let id = runtime
            .created_messages
            .get(&created.created)
            .copied()
            .ok_or_else(|| unresolved_manifest(&created.created))?;
        resources.messages.insert(alias.clone(), id);
    }
    Ok(resources)
}

fn unresolved_manifest(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved manifest ref: {key}"),
    )
}
```

`handle_event` 시그니처: `ruleset_key: &str` + `instances` + `instance_ids` 추가. `from_event(event, ruleset_key)` + strip(16k) 후 `run(&context, &ActionPlan{steps}, mutation, responder, instances, instance_ids)`. defer(16k) 실행도 유지.

- [ ] **Step 3: `validate.rs` — 심볼 테이블(Message/Instance) + RegisterInstance 검사**

`CreatedKind`에 `Message, Instance` 추가. PostPanel arm에서 `created.insert(key.clone(), CreatedKind::Message)`(dup 검사) — PostPanel이 이제 key 보유. RegisterInstance arm:
```rust
                ActionSpec::RegisterInstance { key, kind: _, resources } => {
                    if created.insert(key.clone(), CreatedKind::Instance).is_some() {
                        errors.push(ValidationError::DuplicateActionKey {
                            rule: rule.key.clone(),
                            key: key.clone(),
                        });
                    }
                    check_manifest(&mut errors, rule, &created, resources);
                }
```
`check_manifest` 헬퍼(파일 하단): 세 map을 순회하며 alias 검증(1~32/`[a-zA-Z0-9_-]`/비어있음) + CreatedRef가 심볼 테이블에서 기대 타입(roles→Role, channels→Channel, messages→Message)인지 검사(없음→Unknown*, 타입불일치→*TypeMismatch), + 세 map 전부 비면 EmptyInstanceResources.
새 ValidationError: `EmptyInstanceResources{rule}`, `InvalidResourceAlias{rule, alias}`, `UnknownCreatedMessageRef{rule, key}`, `CreatedMessageRefTypeMismatch{rule, key}`, `UnknownCreatedRoleRef`/`CreatedRoleRefTypeMismatch`(16g 재사용), `UnknownCreatedChannelRef`/`CreatedChannelRefTypeMismatch`(16h 재사용).
> alias 검증 함수는 InstanceId::parse 규칙과 동일(1~32, `[a-zA-Z0-9_-]`) — automation-core 내 로컬 헬퍼로.

- [ ] **Step 4: 호출자 파급(컴파일러 가이드)**

`cargo build/test -p automation-core` 컴파일 에러대로:
- `run(...)` 호출 전부에 `, &instances, &instance_ids` 추가(테스트: `&InMemoryInstanceStore::new()`, `&SequenceInstanceIdGenerator::new("test", 1)`).
- `handle_event(...)` 호출에 `, ruleset_key, &instances, &instance_ids`(테스트: `"test"` + 위 둘).
- `RuntimeContext::from_event(event)` → `from_event(event, "test")`.
- `CreatedResource::{Role,Channel,Message}` 단언에 `key` 필드 추가.
- `PlannedAction::PostPanel`/`ActionSpec::PostPanel` 구성에 `key` 추가.
- policy.rs match에 RegisterInstance no-op arm(catch-all).
대상: tests/{create,run,modal,template,linking,overwrite,post_panel,deferred}.rs + policy.rs.

- [ ] **Step 5: `tests/instance_registration.rs` 신설**

핵심 테스트(스펙 §10): register→store.get(metadata/resources 정확), resolved ids, CreatedResource::Instance 기록(run 결과), DuplicateInstance fail-fast, validate(symbol table 6종), created_messages 해소, 전체 StudyRoom run, 같은 id 다른 guild. (InMemoryInstanceStore + SequenceInstanceIdGenerator 사용.)

- [ ] **Step 6: 게이트 + 커밋**

`cargo build`(automation-core까지; runtime/tool은 Task D) + `cargo test -p automation-core`:
```bash
git add crates/automation-core
git commit -m "feat(automation-core): RegisterInstance interpret/run/validate + registration tests"
```

---

## Task D: runtime/tool 배선 + 전체 게이트 + push

- [ ] **Step 1: `automation-runtime` gateway/runner — store/generator 배선**

`runner::handle_interaction`에 `instances: &impl InstanceStore` + `instance_ids: &impl InstanceIdGenerator` 추가, `handle_event(&event, ruleset, bindings, mutation, &responder, failure_message, ruleset_key, instances, instance_ids)` 전달. `gateway::run`에 `instances`/`instance_ids`를 파라미터로 받거나 내부 생성(tool 제공) — 플랜 결정: **gateway::run이 `instances: impl InstanceStore` + `instance_ids: impl InstanceIdGenerator`를 인자로**(tool이 InMemoryInstanceStore + SequenceInstanceIdGenerator 제공). Cargo.toml에 automation-instance dep.

- [ ] **Step 2: `tools/interaction-smoke` — store/generator + StudyRoom register_instance**

Cargo.toml에 automation-instance dep. main.rs: `InMemoryInstanceStore::new()` + `SequenceInstanceIdGenerator::new("room", 1)` 생성 → `gateway::run(token, ruleset_key, ruleset, bindings, failure_message, store, generator)`. StudyRoom submit 룰에 PostPanel `key: study_welcome_panel` + register_instance(study_room_instance, study_room, manifest) 추가(edit_response 앞).

- [ ] **Step 3~6: 전체 게이트 + push**
- `cargo build`(경고0) / `cargo test`(전체 ~320) / `cargo clippy --all-targets -- -D warnings`(0) / `cargo fmt --all -- --check` / `git push origin main`.
- 커밋: `feat(interaction-smoke): StudyRoom register_instance wiring`

---

## Self-Review (스펙 대비)
- RegisterInstance(created-only manifest) + InstanceResourceRefs + IdGenerator(fail-fast) ✅.
- CreatedResource key+Instance(audit/RunResult), PostPanel key + created_messages, RuntimeContext ruleset_key ✅.
- 실행 순서 불변식(해소→id→register→성공후 binding), Store 실패 fail-fast + orphan 경계 ✅.
- validate 심볼 테이블(Message/Instance) + 타입/forward/alias/empty ✅. output key 단일 namespace ✅.
- metadata 시스템값(guild/ruleset_key/actor/status), action은 key/kind/refs만 ✅.
- automation-instance 순수(Postgres 17d 별도), param threading(bundle 17c) ✅.

## Codex 핸드오프 (4청크)
- **A** = automation-instance IdGenerator + automation-state RegisterInstance/PostPanel key. 커밋 2.
- **B** = automation-core types(CreatedResource/RuntimeContext/PlannedAction) + dep. build 부분. 커밋 1.
- **C** = interpret/run/validate 로직 + 호출자 파급(컴파일러 가이드) + 테스트. 커밋 1.
- **D** = runtime/tool 배선 + 전체 게이트 + push. 커밋 1 + push.
보고: 테스트 수 + 전체 + clippy/fmt + push 해시 + 파급 규모 + 이탈. **live 없음.**
