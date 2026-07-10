# Phase 16i — PostPanel Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16i — created(또는 existing) 채널에 **런타임 환영/컨트롤 패널**(메시지+정적 버튼)을 게시. 순수 Mock.
- **선행**: 16h(ChannelRef, UpsertOverwrite). live/dynamic join은 후속.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. **16i 순수 코어(Mock)** — automation-core/state(+discord-model MessageId). **automation-runtime 무수정**(post_panel seam default-unsupported).

**목표:** created 채널에 `${input.room_name}` 템플릿 메시지 + 정적 버튼 패널을 게시. **버튼은 정적 key** → 기존 `button_click` 룰이 처리. **동적 join은 별개.**

---

## 0. 범위

**포함:** `ActionSpec::PostPanel { channel: ChannelRef, content: String, buttons: Vec<ButtonSpec> }` · content always-template(16e sanitizer 재사용) · 정적 `ButtonSpec` 재사용 · `post_panel` seam(DiscordMutationAdapter, default-unsupported) · `MessageId`(discord-model 신규) · Mock 기록 · PostPanel 버튼 key를 전역 button_keys에 등록(button_click 라우팅) · created 채널 게시 검증 · policy finding.

**제외(→후속):** **dynamic join button** · custom_id에 created role/channel context 싣기 · room instance state/registry · 버튼 클릭이 created role 자동 참조 · created message id 소비 · live Discord · DB.

---

## 1. 왜 join이 아니라 welcome/control인가 (핵심 근거)
16h로 채널을 **비공개**(@everyone deny view)로 만들면 — **그 방 역할이 없는 사람은 채널 자체가 안 보임**. 따라서 채널 **내부**의 "참가하기" 버튼은 **이미 들어온 멤버만** 봄 → **join용으로 부적합**. 그래서 16i PostPanel은 **방 내부 환영/안내/관리 패널**:
```
"스터디룸 '수학'이 생성되었습니다. 멤버끼리 자유롭게 대화하세요."
[도움말]
```
진짜 "참가" 패널은 **공개 허브 채널 + 동적 custom_id(room→role→channel)**가 필요 → 별개 트랙(dynamic instance buttons / public join registry).

---

## 2. 타입

### discord-model (신규)
```rust
define_id!(MessageId);   // ids.rs 매크로 재사용 + lib.rs 재노출
```

### automation-state (스키마)
```rust
use crate::panel::ButtonSpec;   // 재사용(설치 패널과 동일 구조 key+label)

enum ActionSpec {
    ..., UpsertOverwrite {..},
    PostPanel {
        channel: ChannelRef,
        content: String,
        #[serde(default)]
        buttons: Vec<ButtonSpec>,
    },
}
```
> **PanelSpec과 병합 안 함.** PanelSpec = 설치시점·고정 채널·static. PostPanel = 이벤트시점·ChannelRef·template content. `ButtonSpec{key,label}`만 재사용. deny_unknown_fields 유지(내부태그 ActionSpec).

### automation-core
```rust
pub struct PostPanelSpec {              // adapter.rs (CreateChannelSpec 형제)
    pub content: String,
    pub buttons: Vec<ButtonSpec>,
}

enum PlannedAction {
    ..., 
    PostPanel {
        channel: PlannedChannel,        // 16h 재사용
        content: String,                // 렌더 전(run에서 template)
        buttons: Vec<ButtonSpec>,
    },
}

enum CreatedResource {                  // Channel/Role(16f/g)에 Message 추가
    ..., 
    Message { action_index: usize, channel: ChannelId, id: MessageId },
}

// seam (default-unsupported)
async fn post_panel(&self, guild: GuildId, channel: ChannelId, spec: PostPanelSpec) -> Result<MessageId, AdapterError>;
```

---

## 3. interpret / run

- **interpret**: PostPanel.channel → PlannedChannel(16h 방식: Existing 해소/Created 보존). content/buttons 그대로 PlannedAction에 clone.
- **run**: channel 해소(Resolved / Created→created_channels), content 렌더(template + 메시지-content sanitize), `let id = post_panel(guild, channel_id, PostPanelSpec{rendered, buttons}).await?` → **반환 MessageId를 `CreatedResource::Message{action_index, channel, id}`로 기록**(뒤 action이 소비하진 않지만 RunResult에 남김 — MessageId 추가 명분, 후속 edit/delete/rollback).

> **sanitize 컨텍스트:** 16i는 PostPanel.content에 16e message-content sanitizer 재사용. *If future public-message policy requires stricter rules than ephemeral responses, split `SanitizeContext::PublicMessageContent` from `EphemeralMessageContent`.* (16e가 @everyone/@here/mention/길이/제어문자를 이미 방어 — 재사용 안전, 분리는 후속.) `ButtonSpec` derive(Clone/PartialEq/Eq — MutationCall·PlannedAction용)는 플랜에서 panel.rs 확인.

fail-fast: 앞 create 실패 시 미실행. missing input → 렌더 실패 → PostPanel 실패.

---

## 4. validate (16h + 버튼/템플릿)

**button_keys 전역화(2-pass):** 기존 install PanelSpec 버튼 + **모든 PostPanel 버튼**을 하나의 button_keys 집합에 등록(전역 중복 → DuplicateButtonKey). 그래야 `button_click` 룰의 UnknownButtonRef 검사가 PostPanel 버튼도 해소(study_help 예시 성립).

**PostPanel action 검사:**
1. channel Created → 앞선 CreateChannel(check_channel_ref 재사용: UnknownCreatedChannelRef/forward/type).
2. channel Existing → channel_bindings 해소(UnknownChannelRef).
3. content template 문법 + `${input.x}`는 ModalSubmit 컨텍스트여야(check_template 재사용).
4. 버튼 label 비어있음 → **EmptyButtonLabel**.
5. 버튼 수 ≤ **5** → 초과 시 **TooManyPanelButtons**.
6. (버튼 key 중복은 전역 button_keys에서 DuplicateButtonKey로 잡힘.)

새 ValidationError: `EmptyButtonLabel{rule,button}`, `TooManyPanelButtons{rule,count}`. (channel ref는 16h 변형 재사용.)

---

## 5. policy (최소)
- `RuntimeMessagePost { rule }` — 모든 PostPanel(런타임 메시지 생성).
- `RuntimeInteractivePanel { rule }` — buttons 비어있지 않을 때.
PolicyFinding(enum)에 2변형 추가. notice 수준.

---

## 6. StudyRoom 전체 (16i로 create-flow **코드 완성**)
```
trigger: { modal_submit, create_study_modal }
actions:
  - { create_role,      key: study_member_role, name: "${input.room_name} 멤버" }
  - { create_channel,   key: study_channel,      name: "study-${input.room_name}" }
  - { upsert_overwrite, channel: {created: study_channel}, target: everyone,                       deny:  [view_channel] }
  - { upsert_overwrite, channel: {created: study_channel}, target: {role: {created: study_member_role}}, allow: [view_channel] }
  - { grant_role,       role: {created: study_member_role}, target: actor }
  - { post_panel,       channel: {created: study_channel}, content: "스터디룸 '${input.room_name}'이 생성되었습니다.", buttons: [{key: study_help, label: "도움말"}] }
```
버튼 → 모달 → 방이름 → 역할생성 → 채널생성 → 비공개권한 → 역할지급 → **환영 패널** = North Star 1차 데모. (join 없이도 강력.)

---

## 7. 테스트 (16)
1. created 채널에 PostPanel 게시(channel 해소).
2. content `${input.room_name}` 렌더+sanitize.
3. Mock post_panel call 기록(channel, content, buttons).
4. **전체 StudyRoom call 순서**: create_role→create_channel→upsert everyone deny→upsert role allow→grant_role→post_panel.
5. missing input → post_panel 실패.
6. unknown created channel key → validate 실패.
7. CreateRole key를 channel created ref → validate 실패(type).
8. forward created channel ref → validate 실패.
9. 버튼 label empty → validate 실패.
10. 버튼 6개(초과) → validate 실패(TooManyPanelButtons).
11. PostPanel → policy finding(RuntimeMessagePost + RuntimeInteractivePanel).
12. **PostPanel 내부** 버튼 key 중복 → validate 실패(DuplicateButtonKey).
13. **PanelSpec 버튼 key ↔ PostPanel 버튼 key** 중복 → validate 실패.
14. **서로 다른 PostPanel action** 간 버튼 key 중복 → validate 실패.
15. PostPanel 버튼 key를 `button_click` trigger가 정상 참조(UnknownButtonRef 안 남) → validate 통과.
16. post_panel 반환 MessageId가 RunResult(`CreatedResource::Message`)에 기록됨.

핵심 = **4번**(전체 StudyRoom 시퀀스가 Mock에서 정확히), **16번**(MessageId 추가 명분 고정).

---

## 8. seam
`DiscordMutationAdapter::post_panel` default-unsupported / Mock 기록(MutationCall::PostPanel) / MessageId 반환(16i 미소비, live 후속). grant_role/create_*/upsert_overwrite 불변. **automation-runtime 무수정.**

---

## 9. 로드맵
```
16h✅ UpsertOverwrite   16i▶ PostPanel (이 스펙, StudyRoom create-flow 코드 완성)
16j  StudyRoom live smoke (실제 Discord — 방 생성→비공개 채널→환영 패널)
후속 트랙: Dynamic instance buttons → Public join registry → join live
```

---

## 10. Codex 핸드오프 (개요)
1. discord-model: `MessageId` 추가(define_id! + 재노출) + serde 테스트.
2. automation-state: ActionSpec::PostPanel(ChannelRef + content + Vec<ButtonSpec>) + serde 테스트.
3. automation-core: adapter(PostPanelSpec + post_panel default-unsupported), mock(MutationCall::PostPanel + 기록 + next MessageId), plan(PlannedAction::PostPanel), interpret(channel 해소 + content/buttons), run(channel 해소 + content 렌더 + post_panel), validate(button_keys 전역 2-pass + PostPanel channel/content/label/count), policy(RuntimeMessagePost/RuntimeInteractivePanel).
4. **automation-runtime 무수정** 확인. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push. **live/토큰 없음.**

## 최종 정리
16i = PostPanel: created/existing 채널에 런타임 메시지+정적 버튼 게시하는 primitive. 버튼은 정적 key로 기존 button_click 룰이 처리, 전역 button_keys 등록. content는 template+sanitize. seam은 DiscordMutationAdapter(post_panel→MessageId, 기록만). 비공개 채널이라 패널은 welcome/control(join 아님). 16h ChannelRef 재사용. 이걸로 **StudyRoom create-flow 코드 완성** → 16j live.
