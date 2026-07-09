# Layer 2 — Interaction Rule Plane 설계 스펙

- **작성일**: 2026-07-10
- **상태**: 아키텍처 확정 (Phase 16a 구현 대기 — Codex 핸드오프)
- **범위**: Layer 2 전체 비전 + Phase 16a MVP(순수 코어)
- **선행**: Layer 1(Phase 1~15) 완료. 아키텍처 문서 §(interaction/verification).

---

## ⚠️ 최상위 원칙 (Layer 2 안전 불변식)

> **AI는 interaction rule을 설치 시점에만 설계한다.**
> **Runtime은 승인·저장된 rule만 결정론적으로 실행한다.**
> **이벤트 발생 시점에는 AI 판단을 절대 호출하지 않는다.**

이걸 어기면(버튼 클릭 때 LLM에 물어보기) Starring 철학이 통째로 무너진다. **automation-runtime은 LLM client에 의존하지 않는다**(빌드 의존성으로 강제).

---

## 0. 목적

Layer 1이 "서버를 어떤 상태로 만들 것인가"였다면, Layer 2는 **"이 서버에서 어떤 이벤트가 오면 어떻게 반응할 것인가"**. 버튼·역할·채널·모달·메시지를 **primitive/도구**로 두고, AI가 사용자 요청을 해석해 **서버별 Interaction Rule**을 설계 → Core가 검증/정책/미리보기/승인 → Runtime이 결정론적으로 해석·실행.

---

## 1. Layer 1 vs Layer 2

| | Layer 1 | Layer 2 |
|---|---|---|
| 질문 | 서버를 이 상태로 | 이벤트 오면 이렇게 반응 |
| 출력 | 정적 상태(DesiredState) | 동적 규칙(InteractionRuleSet) |
| 적용 | 한 번 reconcile | 상시 이벤트 해석 |
| 위험 | admin 부여(diff에 보임) | 무한 클릭·escalation·입력 주입(클릭 전 안 보임) |

---

## 2. 핵심 안전 불변식
§ 최상위 원칙 재강조. AI는 **설계자**(install-time), Runtime은 **결정론적 interpreter**(event-time). 이벤트-타임 AI 호출 = **금지**.

---

## 3. 자동화 = 데이터, 코드 생성 금지
AI는 **정해진 primitive 어휘 안에서 규칙(데이터)을 조립**한다. **임의 핸들러 코드를 생성하지 않는다.** 액션 어휘가 유한 → policy가 정적 분석 가능, preview 가능, 승인 가능. (Layer 1이 roles/channels로 임의 구조를 조립했듯, Layer 2는 trigger/action으로 임의 규칙을 조립.)

---

## 4. Rule Plane 개념

**Phase 16a는 "인증 버튼 기능"이 아니다. Interaction Rule Plane의 첫 primitive subset이다.** 지원 primitive가 작을 뿐 모델은 범용:
```
InteractionRuleSet → InteractionRule → Trigger → Condition → Action
                                       ↓ (event 발생)
RuntimeEvent → 규칙 lookup → Trigger match → RuntimeContext → ActionPlan → 실행 → Audit
```
"버튼→역할"은 하드코딩 기능이 아니라 이 어휘로 조립된 한 규칙. 인증·스터디룸·티켓·투표 전부 같은 도구상자의 다른 조합. 규칙 집합은 **서버(guild)별**.

---

## 5. Phase 16a MVP 범위

**첫 지원 primitive (모델은 범용, 지원만 이걸로):**
- **Trigger**: `ButtonClick`
- **Action**: `GrantRole`, `RespondEphemeral`
- **Resource**: `Panel`, `Button`, `Role`(참조)
- **Condition**: (선택) `ActorLacksRole` 정도. 없어도 됨.

MVP 대표 규칙: 인증 채널의 verify 버튼 클릭 → actor에게 verified 역할 부여 + ephemeral 응답. **하지만 코드는 범용 rule 엔진.**

---

## 6. 타입 모델 (automation-state)

```
struct InteractionRuleSet { version: u32, panels: Vec<PanelSpec>, rules: Vec<InteractionRule> }
struct PanelSpec { key: String, channel: ResourceKey, content: String, buttons: Vec<ButtonSpec> }
struct ButtonSpec { key: String, label: String }   // custom_id = key

struct InteractionRule { key: String, trigger: TriggerSpec, conditions: Vec<ConditionSpec>, actions: Vec<ActionSpec> }
enum TriggerSpec { ButtonClick { component: String } }          // 16a. (ModalSubmit/MemberJoin 후속)
enum ConditionSpec { ActorLacksRole { role: ResourceKey } }     // 16a 최소(옵션)
enum ActionSpec {
    GrantRole { role: ResourceKey, target: ActionTarget },
    RespondEphemeral { content: String },
}                                                                // (Revoke/CreateChannel/OpenModal 후속)
enum ActionTarget { Actor }                                      // 16a: actor만. (member/role 후속)
```

**런타임 (automation-core):**
```
struct RuntimeEvent { guild_id: GuildId, actor: UserId, kind: EventKind }
enum EventKind { ButtonClick { component: String } }
struct RuntimeContext { guild_id: GuildId, actor: UserId }       // (후속: modal inputs)
struct ActionPlan { steps: Vec<PlannedAction> }
enum PlannedAction {
    GrantRole { role: RoleId, target: UserId },                  // role은 해소된 id
    RespondEphemeral { content: String },
}
```
role key→id 해소는 설치 시점의 binding(Layer 1 resource-resolution 재활용) 또는 전달된 role registry 사용.

---

## 7. validate / compile 책임 (automation-core)
- rule/panel/button **key 유일성**. trigger의 component가 존재하는 button을 가리키는가.
- action이 참조하는 role key가 존재하는가(role registry/DesiredState).
- 지원되지 않는 trigger/action이면 실패.
- → 유효하면 정규화된 RuleSet.

---

## 8. policy 책임 (동작 안전 분석)
Layer 1 policy 철학을 **동작에** 적용: 규칙이 **발동 가능한 액션 집합**을 정적 분석.
- privileged 역할 부여(admin 등) → Deny.
- (후속) 무한 생성·escalation 가드.
16a는 최소: GrantRole의 대상 역할이 privileged면 Deny. RespondEphemeral은 안전.

---

## 9. interpreter 책임 (automation-core, LLM 의존 금지)
`interpret(&RuntimeEvent, &InteractionRuleSet) -> Option<ActionPlan>`:
- event.kind와 매칭되는 rule의 trigger 찾기(ButtonClick component 일치).
- conditions 평가(16a: 옵션).
- actions → PlannedAction(target=actor 해소, role key→id 해소)로 ActionPlan 생성.
- 매칭 rule 없으면 `None`(no-op).
**결정론적. LLM 호출 없음.**

그리고 `run(&ActionPlan, &impl DiscordMutationAdapter, &impl InteractionResponder)` — plan을 seam으로 실행.

---

## 10. Adapter seam 분리 (Mutation vs Responder)

성격이 다르므로 분리:
```
trait DiscordMutationAdapter {   // 일반 Discord REST mutation
    async fn grant_role(&self, guild: GuildId, member: UserId, role: RoleId) -> Result<(), AdapterError>;
    // (revoke_role/create_channel/create_role/send_message 후속)
}
trait InteractionResponder {     // interaction token/callback (시간 제한·응답 계열)
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError>;
    // (open_modal/defer_response 후속)
}
```
> GrantRole은 REST mutation, RespondEphemeral/OpenModal은 **interaction response**(토큰·시간 제한)라 실세계에서 경계가 다름. **16a는 live 아님 → MockMutationAdapter/MockInteractionResponder로 결정론 테스트.** 실제 twilight/gateway/http interaction 연결은 **Phase 16b**.

---

## 11. Mock 테스트 전략 (Phase 16a — 순수·토큰 없음)
1. ButtonClick이 matching rule을 찾는다.
2. verify_button ButtonClick → GrantRole(actor, verified_member) ActionPlan 생성.
3. 매칭 rule 없는 button click → no-op(None).
4. 없는 role key 참조 → validate 실패.
5. duplicate rule/component key → validate 실패.
6. RespondEphemeral action이 plan/실행 결과에 포함.
7. **AI/runtime 분리**: automation-runtime/core가 **ai-gateway/LLM에 의존하지 않음**(빌드/의존성 검사).
+ policy: privileged 역할 GrantRole → Deny.

---

## 12. Phase 16b로 미루는 것 (Forbidden in 16a)
- 실제 Discord **gateway 연결** / **interaction endpoint** 처리
- **DB persistence**(16a는 fixture/파일 RuleSet)
- **modal** / **dynamic template**(`${input.x}`) / dynamic create_channel·create_role action
- **condition expression language**
- retry/backoff, live audit persistence
- **⛔ event-time AI 호출** — `Runtime must not call LLM during interaction handling.`

---

## 13. 장기 비전 (north star — 16a 미지원)

스터디룸 = 같은 도구상자의 더 큰 조합:
```
ButtonClick(create_study) → OpenModal(create_study_modal)
→ ModalSubmit → CreateRole("study_${name}_member") → CreateChannel("study_${name}", private)
→ GrantRole(actor) → PostPanel(join 버튼)
```
필요 확장(후속): ModalSubmit trigger, OpenModal/CreateRole/CreateChannel/PostPanel action, `${input}` 동적 템플릿(+sanitize/bounding), member/role target, gateway 런타임, DB RuleSet. **16a에서는 전부 "미지원"으로 명시**(Codex가 템플릿·모달까지 안 가게).

---

## 14. Codex 핸드오프 (Phase 16a)
1. crate: `automation-state`(스키마) + `automation-core`(validate/policy/interpret/run + seam trait + Mock). automation-runtime의 live는 16b.
2. 지원: ButtonClick trigger / GrantRole·RespondEphemeral action / Panel·Button·Role resource.
3. **자동화-runtime은 ai-gateway/LLM 의존 금지**(의존성 검사 테스트 포함).
4. AdapterError는 executor-core 재사용(또는 자체). seam은 DiscordMutationAdapter/InteractionResponder 분리 + Mock.
5. 완료 게이트: build/test/clippy(-D warnings)/fmt. members 추가. 완료 후 `git push origin main`. **live/토큰/DB/modal/template 없음.**

---

## 최종 정리
Layer 2 = Interaction Rule Plane. 자동화=선언형 데이터. AI=설치시점 설계자. Runtime=저장된 rule의 결정론적 interpreter. **16a=순수 코어 MVP(ButtonClick+GrantRole+RespondEphemeral), 16b=live edge.** DB/Gateway/Modal/Dynamic Template는 후속. → Layer 2도 Layer 1처럼 **검증·승인·예측·안전 실행** 가능.
