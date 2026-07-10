# Phase 16k — Deferred Response Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`). live는 Claude hands-on.

**Goal:** DeferEphemeral(3초 ACK) → mutation → 성공 시 EditResponse(원본 edit "완료") / 실패 시 handle_event fallback(원본 edit "실패"). 풀스택.

**Architecture:** automation-state 액션 2(DeferEphemeral/EditResponse). automation-core: responder seam 2, run arm 2, handle_event가 **defer ACK 성공 추적**(strip 후 나머지 run, 실패 시 failure_message render+edit fallback), validate 6. automation-runtime twilight responder(DeferredChannelMessageWithSource/update_response) + failure_message 배선. tool StudyRoom defer/edit.

## Global Constraints
- **코드 주석 금지.** **Codex 구현, live는 Claude.**
- failure_message = **앱 제공**(tool→gateway→runner→handle_event), core 하드코딩 없음, render+sanitize.
- Defer rule 계약: `[DeferEphemeral(0), ...work..., EditResponse(마지막·정확히 1개)]`.
- 완료 게이트: build(경고0)/test/clippy(`--all-targets -- -D warnings`)/fmt.

---

## Task A: automation-core stack (state + core)

- [ ] **Step 1: `automation-state/rule.rs` — ActionSpec 2 변형 + serde 테스트**

`ActionSpec` enum에 PostPanel 다음 추가:
```rust
    DeferEphemeral,
    EditResponse {
        content: String,
    },
```

test 모듈 끝에:
```rust
    #[test]
    fn defer_and_edit_roundtrip() {
        assert_eq!(
            serde_json::from_str::<ActionSpec>(r#"{"type":"defer_ephemeral"}"#).unwrap(),
            ActionSpec::DeferEphemeral
        );
        assert_eq!(
            serde_json::from_str::<ActionSpec>(r#"{"type":"edit_response","content":"완료"}"#)
                .unwrap(),
            ActionSpec::EditResponse {
                content: "완료".to_string(),
            }
        );
        assert!(serde_json::from_str::<ActionSpec>(
            r#"{"type":"edit_response","content":"x","evil":1}"#
        )
        .is_err());
    }
```
Run: `cargo test -p automation-state` → 커밋 `feat(automation-state): defer/edit response actions`

- [ ] **Step 2: `automation-core/plan.rs` — PlannedAction 2 변형**

`PlannedAction` enum에 PostPanel 다음:
```rust
    DeferEphemeral,
    EditResponse {
        content: String,
    },
```

- [ ] **Step 3: `automation-core/adapter.rs` — InteractionResponder seam 2**

`InteractionResponder` trait의 `open_modal` 다음:
```rust
    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "defer_ephemeral is not supported",
        ))
    }

    async fn edit_response(&self, _content: String) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "edit_response is not supported",
        ))
    }
```

- [ ] **Step 4: `automation-core/mock.rs` — ResponderCall 2 + impl**

`ResponderCall` enum에:
```rust
    DeferEphemeral,
    EditResponse { content: String },
```

`impl InteractionResponder for MockInteractionResponder`의 `open_modal` 다음:
```rust
    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.calls.lock().unwrap().push(ResponderCall::DeferEphemeral);
        Ok(())
    }

    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        self.calls
            .lock()
            .unwrap()
            .push(ResponderCall::EditResponse { content });
        Ok(())
    }
```

- [ ] **Step 5: `automation-core/interpret.rs` — 2 arm**

action match에 PostPanel arm 다음:
```rust
            ActionSpec::DeferEphemeral => {
                steps.push(PlannedAction::DeferEphemeral);
            }
            ActionSpec::EditResponse { content } => {
                steps.push(PlannedAction::EditResponse {
                    content: content.clone(),
                });
            }
```

- [ ] **Step 6: `automation-core/run.rs` — run arm 2 + handle_event 재작성**

run()의 match에 PostPanel arm 다음:
```rust
            PlannedAction::DeferEphemeral => {
                responder.defer_ephemeral().await?;
            }
            PlannedAction::EditResponse { content } => {
                let rendered = render(content, context, SanitizeContext::EphemeralMessageContent)?;
                responder.edit_response(rendered).await?;
            }
```

`handle_event` 함수를 전체 교체:
```rust
pub async fn handle_event(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    mutation: &impl DiscordMutationAdapter,
    responder: &impl InteractionResponder,
    failure_message: &str,
) -> Result<HandleOutcome, AdapterError> {
    match interpret(event, ruleset, bindings) {
        Some(plan) => {
            let context = RuntimeContext::from_event(event);
            let mut steps = plan.steps;
            let defer_acked = if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
                responder.defer_ephemeral().await?;
                steps.remove(0);
                true
            } else {
                false
            };
            match run(&context, &ActionPlan { steps }, mutation, responder).await {
                Ok(_) => Ok(HandleOutcome::Executed),
                Err(error) => {
                    if defer_acked {
                        if let Ok(rendered) = render(
                            failure_message,
                            &context,
                            SanitizeContext::EphemeralMessageContent,
                        ) {
                            let _ = responder.edit_response(rendered).await;
                        }
                    }
                    Err(error)
                }
            }
        }
        None => Ok(HandleOutcome::NoOp),
    }
}
```

- [ ] **Step 7: `automation-core/validate.rs` — 6 error + action arm 2 + Defer/Edit 계약 검사**

`ValidationError`의 `TooManyPanelButtons {...}` 다음:
```rust
    DeferNotFirst {
        rule: String,
    },
    ConflictingInitialResponse {
        rule: String,
    },
    EditResponseWithoutDefer {
        rule: String,
    },
    DeferredMissingEditResponse {
        rule: String,
    },
    MultipleEditResponse {
        rule: String,
    },
    EditResponseNotLast {
        rule: String,
    },
```

action match(`for action in &rule.actions`)에 UpsertOverwrite/PostPanel arm 다음:
```rust
                ActionSpec::DeferEphemeral => {}
                ActionSpec::EditResponse { content } => {
                    check_template(&mut errors, rule, &modal_fields, content);
                }
```

action match 루프 **다음**(같은 `for rule` 블록 안, `created` 사용 끝난 뒤)에 계약 검사 추가:
```rust
        let defer_positions: Vec<usize> = rule
            .actions
            .iter()
            .enumerate()
            .filter(|(_, action)| matches!(action, ActionSpec::DeferEphemeral))
            .map(|(index, _)| index)
            .collect();
        let edit_positions: Vec<usize> = rule
            .actions
            .iter()
            .enumerate()
            .filter(|(_, action)| matches!(action, ActionSpec::EditResponse { .. }))
            .map(|(index, _)| index)
            .collect();
        let has_other_initial = rule.actions.iter().any(|action| {
            matches!(
                action,
                ActionSpec::RespondEphemeral { .. } | ActionSpec::OpenModal { .. }
            )
        });
        let last_index = rule.actions.len().saturating_sub(1);
        if defer_positions.iter().any(|&index| index != 0) {
            errors.push(ValidationError::DeferNotFirst {
                rule: rule.key.clone(),
            });
        }
        if !defer_positions.is_empty() {
            if has_other_initial {
                errors.push(ValidationError::ConflictingInitialResponse {
                    rule: rule.key.clone(),
                });
            }
            if edit_positions.is_empty() {
                errors.push(ValidationError::DeferredMissingEditResponse {
                    rule: rule.key.clone(),
                });
            }
        }
        if !edit_positions.is_empty() && defer_positions.is_empty() {
            errors.push(ValidationError::EditResponseWithoutDefer {
                rule: rule.key.clone(),
            });
        }
        if edit_positions.len() > 1 {
            errors.push(ValidationError::MultipleEditResponse {
                rule: rule.key.clone(),
            });
        }
        if edit_positions.iter().any(|&index| index != last_index) {
            errors.push(ValidationError::EditResponseNotLast {
                rule: rule.key.clone(),
            });
        }
```

- [ ] **Step 8: handle_event 호출자 갱신(테스트 8곳) — 컴파일러 가이드**

`tests/{create,run,modal,template}.rs`의 `handle_event(...)` 호출에 마지막 인자 `""` 추가(이 테스트들은 defer 없음 → fallback 미발동). 컴파일러가 각 site 지시.

- [ ] **Step 9: `automation-core/tests/deferred.rs` 신설 (16k 코어 테스트)**

```rust
use std::collections::BTreeMap;

use automation_core::adapter::AdapterErrorKind;
use automation_core::event::{EventKind, RuntimeContext, RuntimeEvent};
use automation_core::mock::{
    MockInteractionResponder, MockMutationAdapter, ResponderCall,
};
use automation_core::plan::{ActionPlan, PlannedAction};
use automation_core::run::{handle_event, run, HandleOutcome};
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

fn rule(actions: Vec<ActionSpec>) -> InteractionRuleSet {
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

fn defer_rule() -> InteractionRuleSet {
    rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "${input.room_name} 멤버".to_string(),
        },
        ActionSpec::EditResponse {
            content: "스터디룸 '${input.room_name}' 완료".to_string(),
        },
    ])
}

#[test]
fn run_executes_defer_and_edit() {
    let context = RuntimeContext::from_event(&submit("cozy"));
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let steps = vec![
        PlannedAction::DeferEphemeral,
        PlannedAction::EditResponse {
            content: "완료".to_string(),
        },
    ];
    block_on(run(&context, &ActionPlan { steps }, &mutation, &responder)).unwrap();
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "완료".to_string(),
            },
        ]
    );
}

#[test]
fn handle_event_defer_success_edits_completion() {
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let outcome = block_on(handle_event(
        &submit("cozy"),
        &defer_rule(),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
        "실패",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "스터디룸 'cozy' 완료".to_string(),
            },
        ]
    );
}

#[test]
fn handle_event_failure_edits_failure_message() {
    let mutation =
        MockMutationAdapter::failing(automation_core::adapter::AdapterError::new(
            AdapterErrorKind::Forbidden,
            "no",
        ));
    let responder = MockInteractionResponder::new();
    let result = block_on(handle_event(
        &submit("cozy"),
        &defer_rule(),
        &ResourceBindingMap::default(),
        &mutation,
        &responder,
        "스터디룸 '${input.room_name}' 실패",
    ));
    assert_eq!(result.unwrap_err().kind, AdapterErrorKind::Forbidden);
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "스터디룸 'cozy' 실패".to_string(),
            },
        ]
    );
}

#[test]
fn valid_defer_rule_passes() {
    assert!(validate(&defer_rule(), &ResourceBindingMap::default()).is_ok());
}

#[test]
fn defer_not_first_fails() {
    let set = rule(vec![
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
        ActionSpec::DeferEphemeral,
        ActionSpec::EditResponse {
            content: "완료".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DeferNotFirst {
            rule: "r".to_string(),
        }));
}

#[test]
fn conflicting_initial_response_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::RespondEphemeral {
            content: "hi".to_string(),
        },
        ActionSpec::EditResponse {
            content: "완료".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::ConflictingInitialResponse {
            rule: "r".to_string(),
        }));
}

#[test]
fn edit_without_defer_fails() {
    let set = rule(vec![ActionSpec::EditResponse {
        content: "완료".to_string(),
    }]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EditResponseWithoutDefer {
            rule: "r".to_string(),
        }));
}

#[test]
fn deferred_missing_edit_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::DeferredMissingEditResponse {
            rule: "r".to_string(),
        }));
}

#[test]
fn multiple_edit_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::EditResponse {
            content: "a".to_string(),
        },
        ActionSpec::EditResponse {
            content: "b".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::MultipleEditResponse {
            rule: "r".to_string(),
        }));
}

#[test]
fn edit_not_last_fails() {
    let set = rule(vec![
        ActionSpec::DeferEphemeral,
        ActionSpec::EditResponse {
            content: "완료".to_string(),
        },
        ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "x".to_string(),
        },
    ]);
    assert!(validate(&set, &ResourceBindingMap::default())
        .unwrap_err()
        .contains(&ValidationError::EditResponseNotLast {
            rule: "r".to_string(),
        }));
}
```

- [ ] **Step 10: 빌드 + 테스트 + 커밋**

Run: `cargo build -p automation-core` (경고 0) / `cargo test -p automation-core`
```bash
git add crates/automation-state crates/automation-core
git commit -m "feat(automation-core): deferred response lifecycle (defer/edit + fallback)"
```

---

## Task B: automation-runtime — twilight defer/edit + failure_message 배선

- [ ] **Step 1: `responder.rs` — defer_ephemeral + edit_response**

import에 `InteractionResponseType`는 이미 있음. `impl InteractionResponder for TwilightInteractionResponder`의 `open_modal` 다음:
```rust
    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        let response = InteractionResponse {
            kind: InteractionResponseType::DeferredChannelMessageWithSource,
            data: Some(InteractionResponseData {
                flags: Some(MessageFlags::EPHEMERAL),
                ..Default::default()
            }),
        };
        self.send(&response).await
    }

    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        self.http
            .interaction(self.application_id)
            .update_response(&self.interaction_token)
            .content(Some(content.as_str()))
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }
```
> twilight 0.17 `update_response(&token).content(Some(&str))`. content()가 Result 반환형이면(validation) `.content(...)?` 로 `?` 추가 — 컴파일러 지시.

- [ ] **Step 2: `gateway.rs` — failure_message 파라미터 + 배선**

`pub async fn run(token, ruleset_key, ruleset, bindings)`에 파라미터 추가:
```rust
pub async fn run(
    token: String,
    ruleset_key: String,
    ruleset: InteractionRuleSet,
    bindings: ResourceBindingMap,
    failure_message: String,
) {
```
`handle_interaction(&http, &ruleset_key, &mutation, &ruleset, &bindings, &interaction_create.0)` 호출에 `, &failure_message` 추가.

- [ ] **Step 3: `runner.rs` — failure_message 파라미터 + handle_event 전달**

`handle_interaction` 시그니처에 `failure_message: &str` 추가(마지막), `handle_event(&event, ruleset, bindings, mutation, &responder)` → `handle_event(&event, ruleset, bindings, mutation, &responder, failure_message)`.

- [ ] **Step 4: 빌드 + 커밋**

Run: `cargo build -p automation-runtime` (경고 0) / `cargo test -p automation-runtime`
```bash
git add crates/automation-runtime
git commit -m "feat(automation-runtime): twilight defer/edit response + failure_message"
```

---

## Task C: tool StudyRoom defer/edit + 게이트 + push

- [ ] **Step 1: `tools/interaction-smoke/src/main.rs` — StudyRoom defer/edit + failure_message**

submit_study_modal 룰의 actions 첫머리에 `ActionSpec::DeferEphemeral` 추가, 기존 첫 액션이던 RespondEphemeral 제거, 맨 끝에 EditResponse 추가:
```rust
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::CreateRole { .. },
                    ActionSpec::CreateChannel { .. },
                    ActionSpec::UpsertOverwrite { .. },   // everyone deny
                    ActionSpec::UpsertOverwrite { .. },   // role allow
                    ActionSpec::GrantRole { .. },
                    ActionSpec::PostPanel { .. },
                    ActionSpec::EditResponse {
                        content: "스터디룸 '${input.room_name}' 생성 완료! 새 채널을 확인하세요.".to_string(),
                    },
                ],
```
(RespondEphemeral "만들고 있어요"는 제거 — DeferEphemeral이 대신 ACK.)

`gateway::run(...)` 호출에 failure_message 추가:
```rust
    gateway::run(
        token,
        RULESET_KEY.to_string(),
        ruleset,
        ResourceBindingMap::default(),
        "스터디룸 생성에 실패했습니다. 봇 권한 또는 역할 순서를 확인해주세요.".to_string(),
    )
    .await;
```

- [ ] **Step 2~5: 게이트 + push**
- `cargo build` (경고 0) / `cargo test` (전체 ~300; 289 + state 1 + deferred 10) / `cargo clippy --all-targets -- -D warnings` (0) / `cargo fmt --all -- --check` / `git push origin main`.
- 커밋: `feat(interaction-smoke): StudyRoom defer/edit lifecycle`

---

## Self-Review (스펙 대비)
- DeferEphemeral(unit)/EditResponse{content} 액션 + responder seam 2(default-unsupported) + run arm 2 ✅.
- **handle_event: defer ACK 성공(strip) 추적 → 실패 시에만 failure_message render+edit fallback**(defer 자체 실패면 `?`로 return, edit 안 함) ✅.
- validate 6(DeferNotFirst/ConflictingInitialResponse/EditResponseWithoutDefer/DeferredMissingEditResponse/MultipleEditResponse/EditResponseNotLast) + Defer rule 계약 ✅.
- twilight defer=DeferredChannelMessageWithSource, edit=update_response ✅. failure_message 앱 threading(tool→gateway→runner→handle_event) ✅.
- handle_event 호출자 8곳(테스트)+runner 갱신, gateway::run 호출자(tool) 갱신 ✅.
- clippy: matches!/saturating_sub/is_empty ✅.

## Codex 핸드오프 (권장 3청크)
- **청크 A** = Task A(core). state+core, deferred 테스트, handle_event 호출자 갱신. 커밋 2개.
- **청크 B** = Task B(runtime). twilight defer/edit + failure_message 배선. 커밋 1개.
- **청크 C** = Task C(tool + 게이트 + push). 커밋 1개 + push.
**live는 Claude**(재사용 토큰: 처리 중→완료 확인). 보고: 테스트 수 + 전체 + clippy/fmt + push 해시 + 이탈.

## Live (Claude, 코드 push 후)
재사용 토큰으로 smoke: Create study room→모달→제출→**"처리 중"(defer)** → 작업 → **"완료"(edit)** 로 갱신 확인. (실패 케이스는 봇 권한을 일시 낮춰 관찰 가능하나 선택.) cleanup.
